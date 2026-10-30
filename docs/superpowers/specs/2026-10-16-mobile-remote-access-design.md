# Mobile remote access

**Date:** 2026-10-16
**Status:** draft
**Supersedes:** [#240](https://github.com/bakedbean/workspacex/issues/240)
(`wsx serve`: mobile web view for shared workspaces)

## Goal

Drive a wsx running on a home machine from a native, mobile-first phone app,
over Tailscale, without ssh and without a terminal on the phone. The app's
core loop mirrors the Claude mobile app:

1. Browse every repo and its workspaces, with status, recap and PR state.
2. Open a workspace's agent as a **chat**: message bubbles, tool-call cards,
   a live "working…" indicator.
3. Send a prompt, stop the agent, and answer an agent waiting on the user.

This works for every agent kind wsx runs (Claude, Codex, pi, omp, Hermes).
That breadth, plus the cross-repo workspace view, is what this adds over
Claude's `--remote-control` relay, which wsx already enables by default
(`src/agent/remote_control.rs`) and which covers single Claude sessions only.

## Non-goals

- **No terminal emulation on the phone.** Nothing streams PTY bytes to the
  client. Agents that cannot be rendered as a chat get a degraded view, not a
  terminal.
- **No arbitrary CLI passthrough.** The API is typed resources, not
  `argv` forwarding. `wsx remote <name>` (`sh -c`) and `workspace create`
  (runs setup scripts) are deliberately out of scope for v1.
- **No public internet exposure.** Reachability is the tailnet's job. wsx
  does not do TLS, NAT traversal or account management.
- **No multi-user.** One owner, one or more of their devices.

## What already works

Most of the pipeline exists; the work is exposing it, not building it.

- **Workspace data.** `wsx workspace list --json` already emits status, recap,
  agents and cached PR state per workspace. The record shape is
  `SharedWorkspaceRecord` (`src/commands/shared.rs:19`), already consumed
  remotely by the shared-hosts feature over ssh.
- **Locating an agent's conversation.** Each agent instance row records its
  harness session id (`agent_session_id`, `src/data/agents.rs:25`); omp's is
  harvested by `src/app/session_harvest.rs`. That id is the key to the
  agent's transcript file.
- **Parsing transcripts.** `sessionx` parses Claude, Codex and pi session
  logs into turns, tool calls and stop reasons, and detects stalls and
  permission prompts (`WorkspaceEvents::pending_tool_uses`,
  `StopReason::is_awaiting_user`). wsx carries its own parsers for omp and
  Hermes (`src/activity/omp_events.rs`, `src/activity/hermes_events.rs`).
- **Injecting a prompt.** `agent_messages` is a durable queue
  (`Store::enqueue_message`, `src/data/messages.rs:62`).
  `App::drain_agent_messages` (`src/app/messaging.rs:339`) delivers it with
  one in-flight injection per target, cold-spawning a dormant session through
  `ensure_instance_session`. The per-agent write shape (bracketed paste then
  Enter for Claude/Codex, plain text for pi/Hermes/omp) is centralized in
  `submit_writes` (`src/pty/session.rs:800`).
- **An in-process listener pattern.** `src/app/ipc.rs` is a tokio listener
  spawned from `main.rs:112` with a handle on `SharedApp`. The HTTP server
  follows the same shape.

## Gaps

1. **No network listener.** The only listener is a local Unix socket with a
   one-verb protocol (`select <repo> <slug>`).
2. **Transcript events are display strings.** `EventSnapshot` carries
   `display: String`, "pre-formatted, already truncated", plus a coarse
   `EventKind`. That is right for a dashboard line and wrong for a chat:
   it loses full text, markdown, tool inputs and outputs, and message
   identity.
3. **Delivered messages carry a peer-mail banner.** `delivery_banner`
   (`src/app/messaging.rs:14`) prefixes every message, including sender-less
   ones (`[message #id]`). A prompt typed by the owner on their phone should
   arrive exactly as typed, like a prompt typed at the terminal.
4. **No interrupt or answer path.** The only writes into a session are
   submit and insert. There is no "send Esc" or "pick option 2".

## Design

### 1. Server: `wsx serve`, embedded in the TUI

The HTTP server runs **inside the TUI process**, spawned next to the IPC
listener in `main.rs`. That process already owns the PTYs, the delivery
loop and `SharedApp`; putting the server anywhere else means first moving
PTY ownership out of the TUI. A standalone daemon is the long-term shape (see
Open questions), not v1.

Settings, read at startup:

| Setting | Default | Meaning |
|---|---|---|
| `remote_api` | `off` | Master switch. On-values: `true`/`on`/`1`/`yes` (same parser as `remote_control_sandbox`). |
| `remote_api_listen` | *(unset)* | `ip:port` to bind. Unset with `remote_api` on → auto-detect the Tailscale IPv4 (`tailscale ip -4`) on port `7749`. |

Both keys must be added to `known_setting_key()`
(`src/cli/parse/config.rs:7`), or `wsx config get/set/edit` rejects them.

Rules:

- **Never bind a wildcard.** `0.0.0.0`, `::` and any non-tailnet address are
  refused with an error in the log and a footer warning; the server does not
  start. "Tailnet address" means within `100.64.0.0/10` or
  `fd7a:115c:a1e8::/48`. `127.0.0.1` is allowed for development.
- **One TUI serves.** When several TUIs run, the first to bind wins; the
  others log and carry on. The phone talks to whichever holds the port.
- **Framework.** `axum` on the existing tokio runtime, the one new dependency.
  SSE via `axum::response::sse`.

### 2. Authentication

Both checks run on every request.

1. **Tailnet identity.** The peer address is resolved with
   `tailscale whois --json <ip>`, cached per address for 60s. The request is
   accepted only when the peer's `UserProfile.LoginName` equals the local
   node's owner. This stops another user on a shared tailnet.
2. **Device token.** `Authorization: Bearer <token>`. Tokens are 32 random
   bytes, base64url, stored hashed (SHA-256) in a new `remote_devices` table
   (`id, name, token_hash, created_at, last_seen_at`).

Pairing and revocation are CLI-only, so a stolen token cannot mint more:

```
wsx remote-api pair <device-name>   # prints the URL + token, and a QR code of both
wsx remote-api devices [--json]
wsx remote-api revoke <device-name>
```

The QR payload is `wsx://pair?url=http://100.x.y.z:7749&token=…`; the app
registers the `wsx://` scheme. Plain HTTP is acceptable because WireGuard
already encrypts the hop; iOS needs an App Transport Security exception for
the tailnet range.

### 3. API

All JSON. Ids are the store's numeric ids, so they survive renames.

```
GET  /v1/repos
GET  /v1/workspaces/{ws}
GET  /v1/agents/{agent}/transcript?before=<cursor>&limit=<n>
GET  /v1/agents/{agent}/events?after=<cursor>         (SSE)
POST /v1/agents/{agent}/prompt     {"text": "...", "client_id": "<uuid>"}
POST /v1/agents/{agent}/interrupt
POST /v1/agents/{agent}/answer     {"prompt_id": "...", "choice": "..."}
GET  /v1/events                                      (SSE, dashboard-level)
```

- **`/v1/repos`** returns repos, each with `SharedWorkspaceRecord`s plus the
  workspace id and agent instance ids. Hidden workspaces
  (`2026-10-15-workspace-hide-rules-design.md`) are included with
  `hidden: true`; the app decides whether to show them.
- **`/v1/events`** pushes `workspace_changed` / `workspace_removed` /
  `agent_state` whenever the TUI's refresh sees a change, so the list stays
  live without polling.
- **`transcript`** pages backwards from the newest message (see §4).
- **Agent `events`** streams `message` (new or updated chat item), `state`
  (§6) and `prompt_request` (§6). A reconnect passes the last cursor
  (`after`, or SSE's `Last-Event-ID`) and replays from there.
- **`prompt`** returns `202 {"message_id": <agent_messages.id>}`. The
  `client_id` is an idempotency key: a retry after a dropped connection
  returns the original id instead of enqueueing twice.

Errors are `{"error": "<code>", "message": "..."}`. Codes include
`unauthorized`, `not_found`, `agent_unsupported` and `not_serving`.

**Target validation.** Every `{ws}` and `{agent}` is resolved against the
store before anything else runs; an id with no row is `not_found`. Handlers
reach a session only through the resolved instance row (transcript path
from `agent_session_id`, writes through the TUI's own session for that
instance), never through a client-supplied path, tmux session name or file
name. The API therefore cannot touch a process or file wsx does not manage.
This carries over #240's rule that only listed sessions are reachable.

### 4. Transcript read path

**New in `sessionx`: a `transcript` module** that parses a session log into
full-fidelity chat items. It reuses the existing dialect detection; it does
not replace `activity::events`, which stays the dashboard's cheap tail.

```rust
pub struct ChatItem {
    pub id: String,              // stable: the log's message/uuid, or "<offset>"
    pub cursor: u64,             // byte offset of the line that produced it
    pub ts_ms: i64,
    pub body: ChatBody,
}
pub enum ChatBody {
    User { text: String, origin: UserOrigin },   // Typed | PeerMail{from} | Injected
    Assistant { markdown: String },
    ToolCall { call_id: String, name: String, summary: String,
               input: serde_json::Value, status: ToolStatus, output: Option<String> },
    Notice { text: String },     // compaction, model switch, errors
}
```

- **Tool results fold into their call.** A `tool_result` updates the
  matching `ToolCall` (`status`, truncated `output`) and is emitted as a
  `message` update with the same `id`, rather than a new item.
- **Injected noise is classified, not shown.** Doctrine, system reminders and
  hook output are `User { origin: Injected }`; the app collapses them.
  Peer mail is recognized by the `delivery_banner` prefix and becomes
  `PeerMail { from }`, rendered as a distinct bubble.
- **Paging** reads backwards from EOF in 256 KiB chunks until `limit` items
  are found; `before` is a byte cursor. Logs are append-only, so cursors are
  stable. A truncated file (`tail_session` already detects this) resets the
  stream with an `event: reset`.
- **Live tailing** is one tokio task per agent with at least one subscriber,
  polling the file every 500ms from the last offset (the same model as the
  activity tail). It stops when the last subscriber leaves.
- **Coverage.** Claude, Codex and pi go through `sessionx`. omp and Hermes
  need a `ChatItem` adapter next to their existing event parsers in
  `src/activity/`. An agent with no adapter yet returns
  `agent_unsupported`, and the app shows its recap and status only.

### 5. Prompt write path

The phone's prompt goes through the existing queue, so it inherits ordering,
one-at-a-time delivery and cold-spawn.

- **New column `agent_messages.origin`**, `TEXT NOT NULL DEFAULT 'agent'`,
  with values `agent` | `remote_user`. Migration via the store's normal
  versioned migration.
- `delivery_banner` is skipped when `origin = 'remote_user'`: the body is
  written verbatim through `submit_writes`, as if typed at the terminal.
- The handler calls `Store::enqueue_message` and then nudges the delivery
  loop through the same path `poll_external_changes` uses, so delivery does
  not wait for the next DB-change poll.
- **Delivery confirmation is the transcript.** The app shows the bubble as
  "sending" until a `User { origin: Typed }` item with matching text appears
  in the event stream. `agent_messages.delivered_at` backs this up for
  agents whose transcript lags.
- **No idle check.** As at the terminal, a prompt sent while the agent works
  is queued by the harness itself (Claude and Codex accept type-ahead). The
  app shows "agent is working, your message will be read next".

**Interrupt** writes the agent's stop key straight to the PTY. It lives in
a new `interrupt_writes(agent)` next to `submit_writes`, so the per-agent
table has one home. `Esc` is the expected key for Claude and Codex; the
pi, omp and Hermes keys must be confirmed against each harness before
phase 2 (see Open questions). Interrupt bypasses the queue on purpose.

### 6. Agent state and interactive prompts

`state` events carry one of `idle` · `working` · `awaiting_user` ·
`needs_terminal` · `stalled` · `offline`, derived from the same signals the
dashboard uses (`StopReason::is_awaiting_user`, `pending_tool_uses`,
`is_stalled`, PTY liveness).

An agent blocked on a dialog (a permission prompt, AskUserQuestion, plan
approval) is the hard case, because the dialog lives only on the terminal
screen. It ships in two stages.

- **v1: detect and route.** Report `needs_terminal` with what is known: the
  pending tool name and input from `pending_tool_uses`. The app shows "Claude
  wants to run `Bash: cargo test`. Answer at the terminal." Push (§8) makes
  this actionable later.
- **v2: answer from the phone, Claude first.** A `prompt_request` event
  carries `prompt_id`, a question and options; `/answer` maps the choice to
  keystrokes. Options come from the tool input where the log has them
  (AskUserQuestion's `questions[].options`, ExitPlanMode's plan). Generic
  permission prompts are read from the `vt100` screen the TUI already
  maintains, by matching the dialog's numbered-option layout. Screen matching
  is fragile and version-specific, so every parser is gated per agent kind
  and per recognized layout. An unrecognized screen falls back to
  `needs_terminal`, never to a guess.

Yolo-mode workspaces avoid most permission prompts but not questions, so v1
alone is useful.

### 7. Mobile client

A native **SwiftUI** iOS app, in a new repository. iOS first because that is
the owner's device; the API is client-agnostic.

Screens:

1. **Workspaces.** Repos as sections; rows show slug, short recap,
   status pill, PR number and lifecycle color, and an agent-state dot.
   Pull to refresh, live via `/v1/events`. Filters: needs attention,
   working, hidden.
2. **Chat.** One per agent. Markdown assistant bubbles, collapsible
   tool-call cards (name + summary, expand for input and output), collapsed
   injected context, a distinct peer-mail style. A composer with send, and a
   stop button while `working`. A banner for `needs_terminal`.
3. **Workspace detail.** Full recap (goal/state/next), branch, PR link, the
   agent picker for multi-agent workspaces.
4. **Settings.** Paired servers (several homes are allowed), unpair.

Behavior:

- The last transcript page and workspace list are cached on device, so
  opening the app off-tailnet shows stale data marked offline instead of a
  spinner.
- SSE reconnects with backoff and resumes from `Last-Event-ID`.
- The token lives in the Keychain.

### 8. Push notifications (deferred)

"Agent finished" and "agent needs you" alerts while the app is closed need
APNs. APNs needs a sender outside the tailnet that holds an Apple key, which
is a third component with its own trust story. Deferred. The cheap interim is
an optional `remote_api_ntfy_topic` setting: on `awaiting_user` or
`needs_terminal` transitions the TUI POSTs to an ntfy topic, which the ntfy
iOS app delivers. The message carries only `<repo>/<slug> needs input`,
never transcript content.

## Security

The API is equivalent to remote code execution as the owner. A prompt to a
yolo agent can do anything that agent can. The controls, in order:

1. Off by default; enabling it is an explicit setting.
2. Bound only to a tailnet or loopback address; wildcard binds are refused.
3. Peer must be the same tailnet user (`tailscale whois`).
4. Per-device bearer token, stored hashed, revocable from the CLI only.
5. No endpoint runs shell commands, creates workspaces or changes settings,
   and every target id is validated against the store (§3).
6. Every mutating request is logged at `info` with device name, endpoint and
   target. Prompt bodies are not logged.

Tailscale ACLs restricting port `7749` to the owner's devices are recommended
in the docs, not enforced.

## Phasing

| Phase | Ships | Done when |
|---|---|---|
| 1 | Server, auth + pairing, `/repos`, `/events`, transcript + agent events for Claude | The app lists workspaces and reads a live Claude chat over the tailnet |
| 2 | `/prompt` + `origin` column, `/interrupt` | A prompt typed on the phone runs in a Claude agent with no banner |
| 3 | `ChatItem` adapters for Codex, pi, omp, Hermes | Every agent kind renders as chat |
| 4 | `needs_terminal`, ntfy | The phone gets a notification and shows what is blocked |
| 5 | `prompt_request` / `/answer`, Claude first | AskUserQuestion and permission prompts are answerable from the phone |

Phases 1 and 2 are the minimum useful product.

## Testing

- **`sessionx::transcript`**: fixture logs per dialect, covering tool
  result folding, injected/peer-mail classification, backwards paging across
  chunk boundaries, truncation reset.
- **Server**: `axum` handlers tested in-process against `Store::open_in_memory`
  with auth stubbed by injecting a `whois` trait. A separate test asserts
  wildcard binds are refused.
- **Delivery**: `origin = remote_user` skips the banner and still goes
  through `submit_writes` (unit test next to the existing `delivery_banner`
  tests).
- **Manual**: `docs/manual-tests/mobile-remote.md`, covering pairing from the
  QR, prompt round trip on Claude and Codex, interrupt, reconnect after
  network loss, revoked token rejected.

## Alternatives considered

**#240: a web page over tmux.** A standalone `wsx serve` serving one embedded
HTML page: list shared workspaces, show `tmux capture-pane` text, send input
with `tmux send-keys`, refresh by hand. It is much smaller and needs no
running TUI. It was set aside for three reasons:

- **Terminal text is not a chat.** A pane capture is terminal output cut to
  the desktop pane's width, unreadable at phone size. The structured
  transcript (§4) is what makes a mobile-first view possible, and a
  capture-based client would be replaced, not extended.
- **Shared workspaces only.** `send-keys` reaches only tmux-shared agents.
  The message queue (§5) reaches any agent and cold-spawns a dormant one.
- **Pull-only.** No live state, so "which agent needs me" still needs a
  manual refresh.

Kept from #240: the target-validation rule (§3), the `known_setting_key()`
step (§1), and the case for a standalone server (Open questions 1).

**A PWA instead of a native app.** It would remove the second repository and
App Store friction, and the API is client-agnostic, so it stays possible.
Native was chosen for the owner's primary use (smooth chat scrolling,
Keychain token storage, a path to APNs).

## Open questions

1. **Daemon vs TUI.** Embedding means "the TUI must be running at home",
   which `agent send` already requires. #240 showed the cost: tmux-shared
   agents keep running without the TUI, so a standalone `wsx serve` could
   reach them with no dashboard open, and the embedded design gives that up.
   A headless `wsx serve` that owns PTYs (or drives shared agents through
   tmux and the rest through the queue) removes the requirement for every
   agent. It is the priority follow-up after phase 2, not a someday item.
2. **Do pi and omp accept type-ahead?** If not, `/prompt` while `working`
   needs a server-side hold until `idle` for those kinds. The same check
   should confirm each harness's interrupt key.
3. **Workspace creation from the phone.** Useful ("start a workspace for
   this issue"), but it runs setup scripts. It could be added later as an
   explicit endpoint behind its own setting.
4. **Images.** Claude transcripts can contain pasted images. v1 renders a
   placeholder; serving them needs a blob endpoint.
