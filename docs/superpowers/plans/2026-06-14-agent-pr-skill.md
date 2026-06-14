# agent-pr Skill Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a bundled `agent-pr` Claude Code skill that spawns a peer review agent in the current wsx workspace and hands it the branch's review context, installable via `wsx setup install-skill` and invocable from an `agent-pr` pinned-command chip.

**Architecture:** The skill is a markdown recipe (`skills/agent-pr/SKILL.md`) that the primary agent follows; it shells out to existing wsx primitives (`wsx agent add`, `wsx agent send`) and git. The single hard code change is generalizing `src/agent/skill.rs` from one embedded skill to a `BUNDLED_SKILLS` table so the installer ships both `wsx` and `agent-pr`. The pinned command is user config, set via the CLI.

**Tech Stack:** Rust (binary + `include_str!` embedding), the `wsx` CLI, git, markdown skill files.

---

## File Structure

- **Create** `skills/agent-pr/SKILL.md` — the skill recipe. Must exist before `skill.rs` compiles (it's `include_str!`'d).
- **Modify** `src/agent/skill.rs` — replace single-skill consts/paths/targets with a `BUNDLED_SKILLS` table and per-(agent × skill) install targets; migrate tests.
- **Modify** `src/cli.rs` — `setup install-skill` handler: call `install_to(&target)` and name the skill in output.
- **Modify** `README.md` — extend the "Agent skill" section to document `agent-pr` and the pinned command.
- **Config (no code)** — set global `pinned_commands` to add `agent-pr=/agent-pr`.

---

## Task 1: Add the `agent-pr` skill content file

**Files:**
- Create: `skills/agent-pr/SKILL.md`

- [ ] **Step 1: Create the skill file**

Create `skills/agent-pr/SKILL.md` with exactly this content:

````markdown
---
name: agent-pr
description: Use in a wsx workspace to spin up a peer review agent that code-reviews the current branch. Takes the reviewer kind (claude|pi|hermes|codex, default claude); spawns the peer, hands it branch-diff-vs-main context, and has it report findings back to you.
---

# agent-pr

Spin up a peer **review agent** in the current wsx workspace and hand it the
branch's review context. You (the agent invoking this skill) act as the
coordinator: you spawn the reviewer, brief it, and stay available to receive
its findings.

## Argument

A single optional argument: the reviewer **kind**, one of `claude`, `pi`,
`hermes`, `codex`. Defaults to `claude` when omitted (e.g. when fired from the
`agent-pr` pinned chip, which submits `/agent-pr` with no argument).

- `/agent-pr` → spawn a `claude` reviewer
- `/agent-pr codex` → spawn a `codex` reviewer

If an argument is given that is not one of the four kinds, stop and tell the
user the valid kinds. Do not guess.

## Steps

1. **Confirm you are in a wsx workspace.** This skill operates on the *current*
   workspace. Verify `$WSX_WORKSPACE_ID` is set, or that the cwd is under
   `~/.local/state/wsx/worktrees/`. If neither holds, stop and tell the user
   this skill must run inside a wsx workspace.

2. **Resolve the kind** from the argument (default `claude`; validate as above).

3. **Spawn the reviewer peer:**

   ```
   wsx agent add <kind>
   ```

   The command prints `added <label>` — capture `<label>` (e.g. `claude#2`).
   This is the peer you will brief. The new agent shares this worktree and
   branch.

4. **Find your own coordinator label** so the reviewer knows where to send
   findings:

   ```
   wsx agent list
   ```

   The workspace's original agent is marked `(primary)`. Use your own label
   (the one matching `$WSX_AGENT_INSTANCE_ID`, or the primary if you are it).

5. **Gather a short brief** — do NOT paste the whole diff; the reviewer shares
   the worktree and can read it:

   ```
   git branch --show-current
   git log main..HEAD --oneline
   git diff --stat main...HEAD
   ```

6. **Hand off to the reviewer** with a single message:

   ```
   wsx agent send <label> "<brief>"
   ```

   The `<brief>` must instruct the reviewer to:
   - Review the current branch against `main`. Run `git diff main...HEAD`
     itself to see the full change.
   - Produce a **risk assessment** — security, performance, breaking changes,
     edge cases.
   - Produce a **gap analysis** — test coverage, documentation, error handling.
   - Report findings back to the coordinator with
     `wsx agent send <your-label> "<findings>"` when done.

   Include the branch name, commit list, and diff-stat from step 5 so the
   reviewer has orientation without re-deriving it.

7. **Tell the user** the reviewer `<label>` is spawned and working, and that its
   findings will arrive as a `[message from <label>]` in this session.

## Example handoff message

```
wsx agent send claude#2 "You are a code reviewer for this wsx workspace.
Branch: feat/widgets (3 commits, 7 files changed). Review this branch against
main: run \`git diff main...HEAD\` to see the full change. Provide (1) a risk
assessment — security, performance, breaking changes, edge cases; and (2) a gap
analysis — test coverage, documentation, error handling. When done, send your
findings back to me with: wsx agent send claude \"<your findings>\"."
```

## Notes

- All `wsx agent` commands resolve the current workspace automatically from
  `$WSX_WORKSPACE_ID` or the cwd — you do not pass repo/slug.
- `wsx agent send` is asynchronous; the reviewer receives the brief shortly
  after you send it and works in its own pane.
- The reviewer shares your worktree. Reviewing is read-only, so this is normally
  safe, but avoid large edits while the review runs.
````

- [ ] **Step 2: Verify the file exists and is non-empty**

Run: `test -s skills/agent-pr/SKILL.md && head -3 skills/agent-pr/SKILL.md`
Expected: prints the first three lines, starting with `---` and `name: agent-pr`.

- [ ] **Step 3: Commit**

```bash
git add skills/agent-pr/SKILL.md
git commit -m "feat(skill): add agent-pr review skill content"
```

---

## Task 2: Generalize `src/agent/skill.rs` to a bundled-skills table

**Files:**
- Modify: `src/agent/skill.rs`

This refactor is compile-coupled (the struct, install fn, and tests change
together), so implement the module body and its tests in one pass, then run the
suite. The new public surface is `BUNDLED_SKILLS`, an `InstallTarget` carrying
`skill`/`content`, and `install_to(&InstallTarget)`.

- [ ] **Step 1: Replace the consts + `InstallTarget` definition**

Replace the existing `SKILL_CONTENT` const and `InstallTarget` struct (top of the file, after the `use` lines) with:

```rust
/// The wsx skill content, embedded at compile time from `skills/wsx/SKILL.md`.
/// Retained as a named const for tests and any direct call sites.
pub const SKILL_CONTENT: &str = include_str!("../../skills/wsx/SKILL.md");

/// The agent-pr skill content, embedded from `skills/agent-pr/SKILL.md`.
pub const AGENT_PR_SKILL_CONTENT: &str = include_str!("../../skills/agent-pr/SKILL.md");

/// A skill bundled into the binary and installed by `wsx setup install-skill`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BundledSkill {
    /// Directory name under each agent's `skills/` dir (`<dir>/<name>/SKILL.md`).
    pub name: &'static str,
    /// Markdown content embedded at compile time.
    pub content: &'static str,
}

/// Every skill wsx ships. Installed for each detected agent.
pub const BUNDLED_SKILLS: &[BundledSkill] = &[
    BundledSkill {
        name: "wsx",
        content: SKILL_CONTENT,
    },
    BundledSkill {
        name: "agent-pr",
        content: AGENT_PR_SKILL_CONTENT,
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallTarget {
    /// Display name of the agent (`"Claude"`, `"Codex"`, `"Hermes"`).
    pub agent: &'static str,
    /// The bundled skill's directory name (`"wsx"`, `"agent-pr"`).
    pub skill: &'static str,
    /// The content to write for this skill.
    pub content: &'static str,
    /// Destination file (`<skills-dir>/<skill>/SKILL.md`).
    pub path: PathBuf,
}
```

- [ ] **Step 2: Replace the per-agent path helpers with skills-dir helpers**

Replace the four functions `default_claude_install_path`, `default_codex_install_path`, `default_hermes_install_path`, and `default_install_path` with:

```rust
/// Claude's skills directory (`~/.claude/skills`). `None` if no home dir.
pub fn claude_skills_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude").join("skills"))
}

/// Codex's skills directory (`~/.codex/skills`). `None` if no home dir.
pub fn codex_skills_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".codex").join("skills"))
}

/// Hermes's skills directory (`~/.hermes/skills`). `None` if no home dir.
pub fn hermes_skills_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".hermes").join("skills"))
}
```

- [ ] **Step 3: Replace `default_install_targets`**

Replace the existing `default_install_targets` with a version that produces one target per (detected agent × bundled skill):

```rust
/// Install targets for `wsx setup install-skill`: every bundled skill, for
/// every detected agent.
///
/// Claude is always included. Codex is included when `WSX_CODEX_BIN` is set,
/// `codex` is on PATH, or `~/.codex` exists. Hermes is included when
/// `WSX_HERMES_BIN` is set, `hermes` is on PATH, or `~/.hermes` exists.
pub fn default_install_targets() -> Option<Vec<InstallTarget>> {
    let mut agents: Vec<(&'static str, PathBuf)> = vec![("Claude", claude_skills_dir()?)];
    if codex_is_installed() {
        agents.push(("Codex", codex_skills_dir()?));
    }
    if hermes_is_installed() {
        agents.push(("Hermes", hermes_skills_dir()?));
    }
    let mut targets = Vec::new();
    for (agent, dir) in agents {
        for skill in BUNDLED_SKILLS {
            targets.push(InstallTarget {
                agent,
                skill: skill.name,
                content: skill.content,
                path: dir.join(skill.name).join("SKILL.md"),
            });
        }
    }
    Some(targets)
}
```

Leave `codex_is_installed`, `hermes_is_installed`, `binary_on_path`, `is_executable`, and `InstallOutcome` unchanged.

- [ ] **Step 4: Replace `install_to` with a target-aware version**

Replace the existing `install_to(target: &Path)` function with:

```rust
/// Install one bundled skill (`target.content`) to `target.path`. Creates
/// parent directories as needed. Returns `Unchanged` without writing when the
/// file already holds identical content (safe to re-run).
pub fn install_to(target: &InstallTarget) -> Result<InstallOutcome> {
    install_content_to(&target.path, target.content)
}

/// Write `content` to `path`, reporting Created/Updated/Unchanged. Split out so
/// tests can exercise the outcome logic directly.
fn install_content_to(path: &Path, content: &str) -> Result<InstallOutcome> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let outcome = match std::fs::read_to_string(path) {
        Ok(existing) if existing == content => return Ok(InstallOutcome::Unchanged),
        Ok(_) => InstallOutcome::Updated,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => InstallOutcome::Created,
        Err(e) => return Err(Error::Io(e)),
    };
    write_atomic(path, content)?;
    Ok(outcome)
}
```

Leave `write_atomic` unchanged.

- [ ] **Step 5: Migrate the tests**

In the `#[cfg(test)] mod tests`, replace the three install-outcome tests and the Claude-only-targets test, and add an agent-pr frontmatter test. Concretely:

Replace `install_creates_when_missing`, `install_is_idempotent_on_identical_content`, and `install_overwrites_when_content_differs` with these (they now drive `install_content_to` directly):

```rust
    #[test]
    fn install_creates_when_missing() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("deep").join("nested").join("SKILL.md");
        assert_eq!(
            install_content_to(&target, SKILL_CONTENT).unwrap(),
            InstallOutcome::Created
        );
        assert_eq!(std::fs::read_to_string(&target).unwrap(), SKILL_CONTENT);
    }

    #[test]
    fn install_is_idempotent_on_identical_content() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("SKILL.md");
        install_content_to(&target, SKILL_CONTENT).unwrap();
        assert_eq!(
            install_content_to(&target, SKILL_CONTENT).unwrap(),
            InstallOutcome::Unchanged
        );
    }

    #[test]
    fn install_overwrites_when_content_differs() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("SKILL.md");
        std::fs::write(&target, "stale content").unwrap();
        assert_eq!(
            install_content_to(&target, SKILL_CONTENT).unwrap(),
            InstallOutcome::Updated
        );
        assert_eq!(std::fs::read_to_string(&target).unwrap(), SKILL_CONTENT);
    }
```

Add a frontmatter test for the new skill, next to `skill_content_has_frontmatter`:

```rust
    #[test]
    fn agent_pr_skill_has_frontmatter() {
        assert!(
            AGENT_PR_SKILL_CONTENT.starts_with("---\n"),
            "agent-pr skill missing YAML frontmatter"
        );
        assert!(
            AGENT_PR_SKILL_CONTENT.contains("name: agent-pr"),
            "agent-pr skill frontmatter missing name field"
        );
    }
```

Replace `default_targets_include_claude_only_when_codex_is_absent` (it now expects one target per bundled skill) with:

```rust
    #[test]
    fn default_targets_cover_every_bundled_skill_for_claude_only() {
        let mut env = EnvGuard::new();
        let home = TempDir::new().unwrap();
        env.set("HOME", home.path());
        env.set("PATH", "");
        env.remove("WSX_CODEX_BIN");
        env.remove("WSX_HERMES_BIN");

        let targets = default_install_targets().unwrap();

        // Only Claude is detected, but one target per bundled skill.
        assert_eq!(targets.len(), BUNDLED_SKILLS.len());
        assert!(targets.iter().all(|t| t.agent == "Claude"));
        let claude_skills = home.path().join(".claude").join("skills");
        assert!(targets.iter().any(|t| {
            t.skill == "wsx" && t.path == claude_skills.join("wsx").join("SKILL.md")
        }));
        assert!(targets.iter().any(|t| {
            t.skill == "agent-pr"
                && t.path == claude_skills.join("agent-pr").join("SKILL.md")
                && t.content == AGENT_PR_SKILL_CONTENT
        }));
    }
```

The remaining target-detection tests (`..._codex_when_binary_is_on_path`, `..._codex_when_codex_home_exists`, `..._ignore_codex_home_regular_file`, `..._codex_when_codex_bin_env_is_set`, and the four Hermes equivalents) use `.any(...)` and remain valid — leave them as-is. Their codex/hermes path assertions still match because a `wsx`-skill target with the old `.../wsx/SKILL.md` path is still produced.

- [ ] **Step 6: Build and run the module's tests**

Run: `cargo test --lib agent::skill 2>&1 | tail -20`
Expected: all `agent::skill::tests::*` pass, including `agent_pr_skill_has_frontmatter` and `default_targets_cover_every_bundled_skill_for_claude_only`.

- [ ] **Step 7: Commit**

```bash
git add src/agent/skill.rs
git commit -m "refactor(skill): install every bundled skill, add agent-pr to the set"
```

---

## Task 3: Name the skill in the `setup install-skill` output

**Files:**
- Modify: `src/cli.rs` (the `SetupInstallSkill` handler, ~line 1034)

- [ ] **Step 1: Update the install loop**

Replace the body of the `if matches!(action, CliAction::SetupInstallSkill)` block's `for target in targets { ... }` loop with:

```rust
        for target in targets {
            let outcome = crate::agent::skill::install_to(&target)?;
            let path = target.path.display();
            let skill = target.skill;
            match outcome {
                crate::agent::skill::InstallOutcome::Created => {
                    println!("installed {skill} skill for {} to {path}", target.agent);
                }
                crate::agent::skill::InstallOutcome::Updated => {
                    println!("updated {skill} skill for {} at {path}", target.agent);
                }
                crate::agent::skill::InstallOutcome::Unchanged => {
                    println!("{skill} skill for {} already up to date at {path}", target.agent);
                }
            }
        }
```

(The only changes from the current code: `install_to(&target)` instead of `install_to(&target.path)`, and each message now leads with `{skill}` instead of the hardcoded `wsx`.)

- [ ] **Step 2: Build**

Run: `cargo build 2>&1 | tail -5`
Expected: compiles with no errors.

- [ ] **Step 3: Smoke-test the installer into a throwaway HOME**

Run:
```bash
HOME=$(mktemp -d) cargo run -q -- setup install-skill
```
Expected output includes both lines (Codex/Hermes absent in a fresh HOME):
```
installed wsx skill for Claude to .../.claude/skills/wsx/SKILL.md
installed agent-pr skill for Claude to .../.claude/skills/agent-pr/SKILL.md
```

- [ ] **Step 4: Commit**

```bash
git add src/cli.rs
git commit -m "feat(cli): name each skill in install-skill output"
```

---

## Task 4: Document `agent-pr` + the pinned command in the README

**Files:**
- Modify: `README.md` (the "Agent skill" section, ~line 1127)

- [ ] **Step 1: Add a subsection after the existing "Agent skill" paragraphs**

Immediately after the `Idempotent: ...` paragraph that ends the "Agent skill" section (just before `## CLI reference`), insert:

```markdown
#### Bundled skills

`wsx setup install-skill` installs every bundled skill for each detected agent:

- **`wsx`** — drives the wsx CLI (workspace ops, slug-vs-`branch_prefix` naming, cross-repo orchestration).
- **`agent-pr`** — run inside a workspace to spin up a peer review agent. It takes the reviewer kind (`claude` | `pi` | `hermes` | `codex`, default `claude`), spawns it with `wsx agent add`, hands it the branch diff vs `main`, and has it report a risk assessment + gap analysis back via `wsx agent send`.

Pin `agent-pr` to a chip so a review is one click away:

```
wsx config set pinned_commands "agent-pr=/agent-pr"
```

Because chips auto-submit, the chip runs `/agent-pr` (defaulting to a `claude` reviewer); type `/agent-pr codex` manually for a different kind.
```

- [ ] **Step 2: Commit**

```bash
git add README.md
git commit -m "docs: document the agent-pr skill and pinned command"
```

---

## Task 5: Set the global pinned command + full verification

**Files:** none (config + verification only)

- [ ] **Step 1: Append `agent-pr` to the global pinned commands**

Read the current value, then set the combined value (current global value is `pr=/pull-request`; append on a new line, do not clobber):

```bash
wsx config list | grep -i pinned    # confirm current value before changing
wsx config set pinned_commands "$(printf 'pr=/pull-request\nagent-pr=/agent-pr')"
```

Expected: `set pinned_commands (N chars)`.

- [ ] **Step 2: Verify the pinned command parsed**

Run: `wsx config list | grep -i pinned`
Expected: shows the value containing both `pr=/pull-request` and `agent-pr=/agent-pr`.

- [ ] **Step 3: Full verification suite (wsx CI gates on rustfmt)**

Run each and confirm clean:
```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings 2>&1 | tail -5
cargo test 2>&1 | tail -15
```
Expected: `fmt --check` produces no output; clippy reports no warnings; all tests pass.

- [ ] **Step 4: Install the skills locally so `/agent-pr` resolves**

Run: `wsx setup install-skill`
Expected: reports installing/updating both `wsx` and `agent-pr` skills for the detected agents (`~/.claude/skills/agent-pr/SKILL.md` now exists).

- [ ] **Step 5: No commit**

This task changes config and local state only; nothing to commit. The branch is ready for a PR.

---

## Notes for the implementer

- `skills/agent-pr/SKILL.md` MUST exist before Task 2 compiles — `skill.rs` `include_str!`s it. Do Task 1 first.
- Do not touch `write_atomic`, `codex_is_installed`, `hermes_is_installed`, `binary_on_path`, or `is_executable` — they're unchanged.
- The skill is a recipe followed by an agent at runtime; there is no Rust code that parses the `agent-pr` argument. Validation of the kind happens inside the agent per the skill's instructions, mirroring how the existing `wsx` skill is a pure instruction document.
