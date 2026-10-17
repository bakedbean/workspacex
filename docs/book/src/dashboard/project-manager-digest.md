# Project manager digest

The digest is a pane below the workspace list with a card for every ready workspace, grouped by repo. Use it to review what each agent is doing and what's left without attaching to each one.

Press `p` to open it. It opens focused and needs no setup.

Within each repo, cards are ordered blocked first, then waiting, then the rest, oldest activity first.

## What a card shows

- **Header:** workspace name, branch, agent, and the agent's reported status, such as `[blocked 4s]`, with its message. With several agents, it shows the most urgent status.
- **Recap:** the agent's `goal:`, `state:`, and `next:` lines. A workspace without a recap shows `no recap yet`.
- **Facts:** git counts (`↑ahead ↓behind ~modified ?untracked`), the PR chip, `active <age> ago`, and `recap stale` when the agent has been active since its last recap.

Agents write recaps and statuses with `wsx recap` and `wsx status`. See [Status and recap](../agents/status-and-recap.md).

## Keys

| Key                    | Action                                       |
| ---------------------- | -------------------------------------------- |
| `p`                    | Open or close the digest                     |
| `j` / `k` (or arrows)  | Move the selection                           |
| `enter`                | Attach to the workspace                      |
| `/`                    | Filter cards by workspace name               |
| `esc`                  | Clear the filter, or return focus to the list |
| `Tab`                  | Cycle focus: list, reply input, digest       |
| `r`                    | Refresh git and PR data                      |
| `q`                    | Close the digest                             |

`q` closes the digest only while the digest has focus. With the list focused, `q` quits wsx and kills running sessions.
