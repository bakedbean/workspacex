# Prompt Tags Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** From the attached (agent chat) view, pick or name an XML tag, type a body in a small multi-line box, and insert `<tag>\nbody\n</tag>` into the focused agent's composer unsubmitted; remember tags by use count and show the top three as footer chips.

**Architecture:** A `commands::tags` model persisted in the `prompt_tags` setting (one `name=uses` line per tag), a two-stage `Modal::PromptTag` (pick → body) backed by a new pure `TextArea` widget, a `Session::insert_text` write path that wraps the text in a bracketed paste, and a `$tags` bar segment wired through the existing segment registry / provider / composer trio with click routing that mirrors `$pins`.

**Tech Stack:** Rust 2024, ratatui + crossterm, rusqlite settings table, tokio tests with a `cat`-backed PTY session. `cargo test` runs the suite; `cargo clippy --all-targets` and `cargo fmt` gate commits.

**Spec:** `docs/superpowers/specs/2026-09-16-prompt-tags-design.md`

## Global Constraints

- Tag names must match `[A-Za-z_][A-Za-z0-9_.-]*`; anything else is never persisted.
- Insertion never writes a trailing `\r`; the user's own Enter submits.
- Footer shows at most `CHIP_COUNT = 3` tag chips, then the manager chip; the manager chip renders even with zero tags.
- Settings key is exactly `prompt_tags`; segment name is exactly `tags`; leader key is `<`.
- Commit messages: no `Co-Authored-By` / "Generated with" trailers (user preference).
- Every task ends with `cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test` green.
- One deviation from the spec, decided while planning: the remote-attach view (`View::AttachedRemote`) gets the footer chips and `^x <` too. Its PTY is an ssh hop into the remote agent, and the insert is plain bytes (exactly how `$pins` already works there), so excluding it would cost more code than including it. Task 8 updates the spec's non-goals line.

---

### Task 1: `commands::tags` model and the `prompt_tags` config key

**Files:**
- Create: `src/commands/tags.rs`
- Modify: `src/commands/mod.rs:5-15` (module doc + `pub mod tags;`)
- Modify: `src/cli/parse/config.rs:30` (allowlist)
- Test: `src/commands/tags.rs` (inline `mod tests`), `src/cli/tests.rs:665`

**Interfaces:**
- Produces:
  ```rust
  pub struct PromptTag { pub name: String, pub uses: u32 }   // Debug, Clone, PartialEq, Eq
  pub const CHIP_COUNT: usize = 3;
  pub const SETTING_KEY: &str = "prompt_tags";
  pub fn is_valid_name(name: &str) -> bool;
  pub fn parse(text: &str) -> Vec<PromptTag>;                 // sorted
  pub fn serialize(tags: &[PromptTag]) -> String;
  pub fn sort(tags: &mut [PromptTag]);
  pub fn bump(tags: &mut Vec<PromptTag>, name: &str);        // re-sorts
  pub fn remove(tags: &mut Vec<PromptTag>, name: &str) -> bool;
  pub fn wrap(name: &str, body: &str) -> String;
  pub fn load(store: &crate::data::store::Store) -> crate::error::Result<Vec<PromptTag>>;  // Err only on a store failure; callers that go on to `save` must NOT treat Err as empty
  pub fn save(store: &crate::data::store::Store, tags: &[PromptTag]) -> crate::error::Result<()>;
  ```

- [ ] **Step 1: Write the failing tests**

Create `src/commands/tags.rs` with only the tests module for now:

```rust
//! Prompt tags: XML tag names the attached view wraps a typed body in
//! (`<context>…</context>`), remembered with a use count so the most-used
//! ones surface first. Persisted in the `prompt_tags` setting as one
//! `name=uses` line per tag, the same plain-text shape as `pinned_commands`.

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(name: &str, uses: u32) -> PromptTag {
        PromptTag {
            name: name.into(),
            uses,
        }
    }

    #[test]
    fn valid_names_follow_the_xml_name_subset() {
        for ok in ["context", "_x", "a.b-c_1", "Task"] {
            assert!(is_valid_name(ok), "{ok}");
        }
        for bad in ["", "1abc", "-x", "has space", "a<b", "a>b", "a/b", "é"] {
            assert!(!is_valid_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn parse_reads_name_equals_uses_and_bare_names() {
        assert_eq!(
            parse("context=3\ntask\n"),
            vec![tag("context", 3), tag("task", 0)]
        );
    }

    #[test]
    fn parse_sorts_by_uses_desc_then_name() {
        assert_eq!(
            parse("b=1\nc=5\na=1\n"),
            vec![tag("c", 5), tag("a", 1), tag("b", 1)]
        );
    }

    #[test]
    fn parse_trims_skips_blanks_and_drops_invalid_lines() {
        assert_eq!(
            parse("  context = 2 \n\n=4\nbad name=1\nx=notanumber\n"),
            vec![tag("context", 2)]
        );
    }

    #[test]
    fn parse_keeps_the_last_duplicate() {
        assert_eq!(parse("a=1\na=7\n"), vec![tag("a", 7)]);
    }

    #[test]
    fn serialize_round_trips_in_sorted_order() {
        let tags = vec![tag("a", 1), tag("c", 5)];
        assert_eq!(serialize(&tags), "c=5\na=1\n");
        assert_eq!(parse(&serialize(&tags)), vec![tag("c", 5), tag("a", 1)]);
        assert_eq!(serialize(&[]), "");
    }

    #[test]
    fn bump_increments_or_inserts_and_resorts() {
        let mut tags = vec![tag("a", 2), tag("b", 2)];
        bump(&mut tags, "b");
        assert_eq!(tags, vec![tag("b", 3), tag("a", 2)]);
        bump(&mut tags, "new");
        assert_eq!(tags, vec![tag("b", 3), tag("a", 2), tag("new", 1)]);
    }

    #[test]
    fn remove_reports_whether_anything_went() {
        let mut tags = vec![tag("a", 1)];
        assert!(remove(&mut tags, "a"));
        assert!(tags.is_empty());
        assert!(!remove(&mut tags, "a"));
    }

    #[test]
    fn wrap_puts_the_body_on_its_own_lines() {
        assert_eq!(wrap("context", "hello"), "<context>\nhello\n</context>");
        assert_eq!(wrap("t", "a\nb"), "<t>\na\nb\n</t>");
        // Trailing newlines in the body do not double up before the close.
        assert_eq!(wrap("t", "a\n\n"), "<t>\na\n</t>");
    }

    #[test]
    fn load_and_save_go_through_the_settings_table() {
        let store = crate::data::store::Store::open_in_memory().unwrap();
        assert!(load(&store).unwrap().is_empty());
        save(&store, &[tag("context", 4), tag("task", 9)]).unwrap();
        assert_eq!(
            store.get_setting(SETTING_KEY).unwrap().as_deref(),
            Some("task=9\ncontext=4\n")
        );
        assert_eq!(load(&store).unwrap(), vec![tag("task", 9), tag("context", 4)]);
    }
}
```

Add to `src/cli/tests.rs` right after `config_set_accepts_pinned_commands_key` (line 665):

```rust
#[test]
fn config_set_accepts_prompt_tags_key() {
    let a = parse(&["config", "set", "prompt_tags", "context=3"]).unwrap();
    match a {
        CliAction::ConfigSet { key, .. } => assert_eq!(key, "prompt_tags"),
        other => panic!("unexpected: {other:?}"),
    }
}
```

(Copy the exact match shape from the pinned test above it if `ConfigSet`'s fields differ.)

- [ ] **Step 2: Register the module and run the tests to see them fail**

In `src/commands/mod.rs` add `pub mod tags;` after `pub mod shared_hosts;` and extend the module doc: "`tags` holds the prompt-tag names the attached view wraps a body in (`<context>…</context>`) and their use counts;".

Run: `cargo test commands::tags` and `cargo test config_set_accepts_prompt_tags_key`
Expected: compile errors — `PromptTag`, `parse`, etc. not found; the CLI test fails because `prompt_tags` is not an allowed key.

- [ ] **Step 3: Implement the module**

Above the tests module in `src/commands/tags.rs`:

```rust
use crate::data::store::Store;
use crate::error::Result;

/// How many tag chips the attached footer shows before the manager chip.
pub const CHIP_COUNT: usize = 3;
/// The settings-table key holding the serialized list.
pub const SETTING_KEY: &str = "prompt_tags";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptTag {
    /// The XML tag name, already validated by `is_valid_name`.
    pub name: String,
    /// How many times a body has been inserted under this tag.
    pub uses: u32,
}

/// A safe subset of XML `Name`: ASCII letter or `_` first, then ASCII
/// letters, digits, `_`, `.`, `-`. Keeps the inserted markup unambiguous
/// and the `name=uses` file format free of `=`, whitespace and `<>`.
pub fn is_valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

/// Parse the `prompt_tags` setting. One `name=uses` per line; a bare `name`
/// has zero uses. Lines that are blank, have an invalid name, or a
/// non-numeric count are dropped. A repeated name keeps its last line.
/// The result is in display order (see `sort`).
pub fn parse(text: &str) -> Vec<PromptTag> {
    let mut tags: Vec<PromptTag> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let (name, uses) = match line.split_once('=') {
            Some((lhs, rhs)) => {
                let Ok(n) = rhs.trim().parse::<u32>() else {
                    continue;
                };
                (lhs.trim(), n)
            }
            None => (line, 0),
        };
        if !is_valid_name(name) {
            continue;
        }
        match tags.iter_mut().find(|t| t.name == name) {
            Some(existing) => existing.uses = uses,
            None => tags.push(PromptTag {
                name: name.to_string(),
                uses,
            }),
        }
    }
    sort(&mut tags);
    tags
}

/// The inverse of `parse`, in display order, one trailing newline per tag.
pub fn serialize(tags: &[PromptTag]) -> String {
    let mut sorted = tags.to_vec();
    sort(&mut sorted);
    sorted
        .iter()
        .map(|t| format!("{}={}\n", t.name, t.uses))
        .collect()
}

/// Display order: most used first, ties by name.
pub fn sort(tags: &mut [PromptTag]) {
    tags.sort_by(|a, b| b.uses.cmp(&a.uses).then_with(|| a.name.cmp(&b.name)));
}

/// Count one more use of `name`, adding it at one use if it is new.
pub fn bump(tags: &mut Vec<PromptTag>, name: &str) {
    match tags.iter_mut().find(|t| t.name == name) {
        Some(t) => t.uses = t.uses.saturating_add(1),
        None => tags.push(PromptTag {
            name: name.to_string(),
            uses: 1,
        }),
    }
    sort(tags);
}

/// Drop `name`; `true` if it was present.
pub fn remove(tags: &mut Vec<PromptTag>, name: &str) -> bool {
    let before = tags.len();
    tags.retain(|t| t.name != name);
    tags.len() != before
}

/// The text inserted into the agent: the opening tag, the body, and the
/// closing tag each on their own line. Trailing newlines in the body are
/// folded so the closing tag never sits under a blank line.
pub fn wrap(name: &str, body: &str) -> String {
    format!("<{name}>\n{}\n</{name}>", body.trim_end_matches('\n'))
}

/// The saved list. A store error propagates rather than reading as "no
/// tags": a caller that loads, edits and saves would otherwise wipe the
/// list on a transient read failure. Render paths that only display may
/// `unwrap_or_default()`.
pub fn load(store: &Store) -> Result<Vec<PromptTag>> {
    Ok(store
        .get_setting(SETTING_KEY)?
        .map(|s| parse(&s))
        .unwrap_or_default())
}

pub fn save(store: &Store, tags: &[PromptTag]) -> Result<()> {
    store.set_setting(SETTING_KEY, &serialize(tags))
}
```

In `src/cli/parse/config.rs` add `| "prompt_tags"` directly after `| "pinned_commands"` (line 30).

- [ ] **Step 4: Run the tests**

Run: `cargo test commands::tags && cargo test config_set_accepts`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add src/commands/tags.rs src/commands/mod.rs src/cli/parse/config.rs src/cli/tests.rs
git commit -m "Prompt tags: name=uses model behind the prompt_tags setting"
```

---

### Task 2: `insert_writes` and `Session::insert_text`

**Files:**
- Modify: `src/pty/session.rs:652-688` (beside `submit_writes`), and the `impl Session` block that holds `scroll_to_live` (line 270)
- Test: `src/pty/session.rs` tests module (beside `submit_writes_wraps_codex_in_bracketed_paste`, line 1508)

**Interfaces:**
- Produces:
  ```rust
  pub(crate) fn insert_writes(agent: AgentKind, text: &str) -> Vec<u8>;
  impl Session { pub async fn insert_text(&self, text: &str) -> bool; }
  ```

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `src/pty/session.rs`, after `submit_writes_keeps_other_agents_plain`:

```rust
    #[test]
    fn insert_writes_wraps_every_agent_in_a_bracketed_paste_without_a_cr() {
        // An insert must land in the composer unsubmitted with its newlines
        // intact. Bracketed paste is how every harness distinguishes a
        // pasted newline from Enter, so the wrapper is unconditional here
        // (unlike `submit_writes`, which needs the CR to read as Enter).
        for agent in AgentKind::ALL {
            let bytes = insert_writes(agent, "<t>\nbody\n</t>");
            assert_eq!(bytes, b"\x1b[200~<t>\nbody\n</t>\x1b[201~".to_vec(), "{agent:?}");
            assert!(!bytes.ends_with(b"\r"));
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn insert_text_reports_failure_when_the_writer_is_gone() {
        // `Session::fake` drops its receiver, which is the same closed-channel
        // state as an exited agent; a lost insert must not read as delivered
        // (the caller bumps the use count only on `true`).
        let s = Session::fake(SessionStatus::Running { pid: 1 });
        assert!(!s.insert_text("<t>\nx\n</t>").await);
    }
```

If `AgentKind::ALL` does not exist, use the explicit list `[AgentKind::Claude, AgentKind::Codex, AgentKind::Pi, AgentKind::Hermes, AgentKind::Omp]` (check `grep -n "ALL" src/pty/session.rs`).

- [ ] **Step 2: Run to see them fail**

Run: `cargo test insert_writes && cargo test insert_text_reports`
Expected: compile error — `insert_writes` / `insert_text` not found.

- [ ] **Step 3: Implement**

Directly after `submit_writes` in `src/pty/session.rs`:

```rust
/// The single write used to drop `text` into an agent's composer WITHOUT
/// submitting it (the prompt-tag insert). Wrapped in a bracketed paste for
/// every agent so embedded newlines stay newlines rather than reading as
/// Enter. omp renders an accepted paste as a `[Paste #N]` placeholder in its
/// editor, which should be acceptable here — the text still submits when
/// the user presses Enter — and is preferable to plain `\n`, which its
/// editor treats as submit. The live omp check is the manual test in
/// `docs/manual-tests/prompt-tags.md`; if it disagrees, this is the one
/// place to give omp a different shape.
pub(crate) fn insert_writes(_agent: AgentKind, text: &str) -> Vec<u8> {
    let mut body = Vec::with_capacity(text.len() + 12);
    body.extend_from_slice(b"\x1b[200~");
    body.extend_from_slice(text.as_bytes());
    body.extend_from_slice(b"\x1b[201~");
    body
}
```

In `impl Session`, next to `scroll_to_live`:

```rust
    /// Insert `text` into the agent's composer unsubmitted (see
    /// `insert_writes`). Returns `false` when the writer channel is closed —
    /// the agent has exited — so callers can avoid recording a use that
    /// never landed.
    pub async fn insert_text(&self, text: &str) -> bool {
        self.scroll_to_live();
        self.writer
            .send(WriteReq::Bytes(insert_writes(self.agent, text)))
            .await
            .is_ok()
    }
```

The `_agent` parameter is kept so the per-agent shape can change in one place if the omp check in Task 8 disagrees with the comment; adjust the comment to what was observed.

- [ ] **Step 4: Run the tests**

Run: `cargo test insert_writes && cargo test insert_text_reports`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add src/pty/session.rs
git commit -m "Session: insert_text writes a bracketed paste without submitting"
```

---

### Task 3: `TextArea` widget

**Files:**
- Create: `src/ui/modal/textarea.rs`
- Modify: `src/ui/modal/mod.rs:12-30` (`mod textarea;` + `pub use textarea::TextArea;`)
- Test: inline `mod tests` in `src/ui/modal/textarea.rs`

**Interfaces:**
- Produces:
  ```rust
  #[derive(Debug, Clone, Default, PartialEq, Eq)]
  pub struct TextArea { /* private */ }
  impl TextArea {
      pub fn new() -> Self;
      pub fn text(&self) -> String;
      pub fn is_blank(&self) -> bool;
      pub fn cursor(&self) -> (usize, usize);          // (row, col) in chars
      pub fn insert_char(&mut self, c: char);
      pub fn insert_str(&mut self, s: &str);           // '\n' splits lines
      pub fn newline(&mut self);
      pub fn backspace(&mut self);
      pub fn delete(&mut self);
      pub fn move_left(&mut self); pub fn move_right(&mut self);
      pub fn move_up(&mut self);   pub fn move_down(&mut self);
      pub fn home(&mut self);      pub fn end(&mut self);
      pub fn render(&self, f: &mut Frame, area: Rect, theme: &Theme);
  }
  ```

- [ ] **Step 1: Write the failing tests**

Create `src/ui/modal/textarea.rs` with just the tests module:

```rust
//! A minimal multi-line text box for modals: char-indexed lines, a cursor,
//! and a soft-wrapping renderer that keeps the cursor in view. No vim keys,
//! no selection — the prompt-tag body box is its only user.

#[cfg(test)]
mod tests {
    use super::*;

    fn with(text: &str) -> TextArea {
        let mut t = TextArea::new();
        t.insert_str(text);
        t
    }

    #[test]
    fn starts_empty_and_blank() {
        let t = TextArea::new();
        assert_eq!(t.text(), "");
        assert!(t.is_blank());
        assert_eq!(t.cursor(), (0, 0));
    }

    #[test]
    fn insert_str_splits_on_newlines_and_leaves_the_cursor_at_the_end() {
        let t = with("ab\ncd");
        assert_eq!(t.text(), "ab\ncd");
        assert_eq!(t.cursor(), (1, 2));
        assert!(!t.is_blank());
        assert!(with("  \n\t").is_blank());
    }

    #[test]
    fn newline_splits_the_current_line_at_the_cursor() {
        let mut t = with("abcd");
        t.move_left();
        t.move_left();
        t.newline();
        assert_eq!(t.text(), "ab\ncd");
        assert_eq!(t.cursor(), (1, 0));
    }

    #[test]
    fn backspace_joins_lines_at_a_line_start_and_is_a_noop_at_the_origin() {
        let mut t = with("ab\ncd");
        t.home();
        t.backspace();
        assert_eq!(t.text(), "abcd");
        assert_eq!(t.cursor(), (0, 2));
        let mut t = with("x");
        t.home();
        t.backspace();
        assert_eq!(t.text(), "x");
    }

    #[test]
    fn delete_removes_under_the_cursor_and_joins_at_a_line_end() {
        let mut t = with("ab\ncd");
        t.move_up();
        t.end();
        t.delete();
        assert_eq!(t.text(), "abcd");
        t.home();
        t.delete();
        assert_eq!(t.text(), "bcd");
    }

    #[test]
    fn up_and_down_remember_the_goal_column_across_short_lines() {
        let mut t = with("abcdef\nx\nabcdef");
        // cursor at (2, 6)
        t.move_up();
        assert_eq!(t.cursor(), (1, 1));
        t.move_up();
        assert_eq!(t.cursor(), (0, 6), "goal column survives the short line");
        t.move_down();
        t.move_down();
        assert_eq!(t.cursor(), (2, 6));
        t.move_up();
        t.move_left();
        t.move_down();
        assert_eq!(t.cursor(), (2, 0), "a horizontal move resets the goal to the current column");
    }

    #[test]
    fn horizontal_moves_wrap_across_line_boundaries() {
        let mut t = with("ab\ncd");
        t.home();
        t.move_left();
        assert_eq!(t.cursor(), (0, 2));
        t.move_right();
        assert_eq!(t.cursor(), (1, 0));
    }

    #[test]
    fn multibyte_chars_count_as_one_column() {
        let mut t = with("héllo");
        assert_eq!(t.cursor(), (0, 5));
        t.move_left();
        t.move_left();
        t.move_left();
        t.move_left();
        t.delete();
        assert_eq!(t.text(), "hllo");
    }

    #[test]
    fn wrap_rows_soft_wrap_at_width_and_locate_the_cursor() {
        let t = with("abcdefgh\nij");
        // width 3: "abc" "def" "gh" | "ij"
        let (rows, (crow, ccol)) = t.wrap_rows(3);
        assert_eq!(rows, vec!["abc", "def", "gh", "ij"]);
        assert_eq!((crow, ccol), (3, 2));
        let mut t = with("abcdef");
        t.home();
        t.move_right();
        t.move_right();
        t.move_right();
        // col 3 on a width-3 line is the START of the second visual row
        assert_eq!(t.wrap_rows(3).1, (1, 0));
    }
}
```

- [ ] **Step 2: Register and run to see them fail**

In `src/ui/modal/mod.rs` add `mod textarea;` to the module list and `pub use textarea::TextArea;` under the other `pub use` lines.

Run: `cargo test ui::modal::textarea`
Expected: compile error — `TextArea` not found.

- [ ] **Step 3: Implement**

Above the tests module:

```rust
use crate::ui::theme::Theme;
use ratatui::layout::Rect;
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextArea {
    lines: Vec<String>,
    row: usize,
    /// Cursor column in CHARS (not bytes), so multi-byte input behaves.
    col: usize,
    /// The column ↑/↓ aim for; set by a vertical move, cleared by any other
    /// edit so a horizontal move re-anchors it.
    goal_col: Option<usize>,
}

impl Default for TextArea {
    fn default() -> Self {
        Self::new()
    }
}

fn char_len(s: &str) -> usize {
    s.chars().count()
}

/// Byte offset of char index `col` in `s` (or `s.len()` past the end).
fn byte_at(s: &str, col: usize) -> usize {
    s.char_indices().nth(col).map(|(i, _)| i).unwrap_or(s.len())
}

impl TextArea {
    pub fn new() -> Self {
        Self {
            lines: vec![String::new()],
            row: 0,
            col: 0,
            goal_col: None,
        }
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn is_blank(&self) -> bool {
        self.lines.iter().all(|l| l.trim().is_empty())
    }

    pub fn cursor(&self) -> (usize, usize) {
        (self.row, self.col)
    }

    fn line(&self) -> &String {
        &self.lines[self.row]
    }

    pub fn insert_char(&mut self, c: char) {
        if c == '\n' {
            self.newline();
            return;
        }
        let at = byte_at(self.line(), self.col);
        self.lines[self.row].insert(at, c);
        self.col += 1;
        self.goal_col = None;
    }

    pub fn insert_str(&mut self, s: &str) {
        for c in s.chars() {
            self.insert_char(c);
        }
    }

    pub fn newline(&mut self) {
        let at = byte_at(self.line(), self.col);
        let rest = self.lines[self.row].split_off(at);
        self.lines.insert(self.row + 1, rest);
        self.row += 1;
        self.col = 0;
        self.goal_col = None;
    }

    pub fn backspace(&mut self) {
        if self.col > 0 {
            let at = byte_at(self.line(), self.col - 1);
            self.lines[self.row].remove(at);
            self.col -= 1;
        } else if self.row > 0 {
            let tail = self.lines.remove(self.row);
            self.row -= 1;
            self.col = char_len(self.line());
            self.lines[self.row].push_str(&tail);
        }
        self.goal_col = None;
    }

    pub fn delete(&mut self) {
        if self.col < char_len(self.line()) {
            let at = byte_at(self.line(), self.col);
            self.lines[self.row].remove(at);
        } else if self.row + 1 < self.lines.len() {
            let tail = self.lines.remove(self.row + 1);
            self.lines[self.row].push_str(&tail);
        }
        self.goal_col = None;
    }

    pub fn move_left(&mut self) {
        if self.col > 0 {
            self.col -= 1;
        } else if self.row > 0 {
            self.row -= 1;
            self.col = char_len(self.line());
        }
        self.goal_col = None;
    }

    pub fn move_right(&mut self) {
        if self.col < char_len(self.line()) {
            self.col += 1;
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = 0;
        }
        self.goal_col = None;
    }

    fn move_vertical(&mut self, target_row: usize) {
        let goal = self.goal_col.unwrap_or(self.col);
        self.row = target_row;
        self.col = goal.min(char_len(self.line()));
        self.goal_col = Some(goal);
    }

    pub fn move_up(&mut self) {
        if self.row > 0 {
            self.move_vertical(self.row - 1);
        }
    }

    pub fn move_down(&mut self) {
        if self.row + 1 < self.lines.len() {
            self.move_vertical(self.row + 1);
        }
    }

    pub fn home(&mut self) {
        self.col = 0;
        self.goal_col = None;
    }

    pub fn end(&mut self) {
        self.col = char_len(self.line());
        self.goal_col = None;
    }

    /// Soft-wrap every line at `width` chars. Returns the visual rows and
    /// the cursor's `(visual row, visual col)`. A cursor exactly at a wrap
    /// boundary sits at the start of the next visual row, so typing at the
    /// end of a full row continues on the row below.
    fn wrap_rows(&self, width: usize) -> (Vec<String>, (usize, usize)) {
        let width = width.max(1);
        let mut rows = Vec::new();
        let mut cursor = (0, 0);
        for (i, line) in self.lines.iter().enumerate() {
            let chars: Vec<char> = line.chars().collect();
            let first_row = rows.len();
            if chars.is_empty() {
                rows.push(String::new());
            } else {
                for chunk in chars.chunks(width) {
                    rows.push(chunk.iter().collect());
                }
            }
            if i == self.row {
                let vrow = first_row + self.col / width;
                let vcol = self.col % width;
                // `col == len` on a line that filled its last row exactly
                // lands one row past what `chunks` produced; make it exist.
                if vrow >= rows.len() {
                    rows.push(String::new());
                }
                cursor = (vrow, vcol);
            }
        }
        (rows, cursor)
    }

    /// Draw the box into `area` and place the terminal cursor. Scrolls so
    /// the cursor row is always visible, keeping it on the last row once
    /// the text is taller than the area.
    pub fn render(&self, f: &mut Frame, area: Rect, theme: &Theme) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let (rows, (crow, ccol)) = self.wrap_rows(area.width as usize);
        let height = area.height as usize;
        let top = crow.saturating_sub(height - 1);
        let visible: Vec<Line> = rows
            .iter()
            .skip(top)
            .take(height)
            .map(|r| Line::from(Span::raw(r.clone())))
            .collect();
        f.render_widget(Paragraph::new(visible).style(Style::default().fg(theme.path)), area);
        f.set_cursor_position((
            area.x + ccol as u16,
            area.y + (crow - top) as u16,
        ));
    }
}
```

`theme.path` is the muted body colour other modals use for values; keep it unless `Theme` gains a dedicated input colour.

- [ ] **Step 4: Run the tests**

Run: `cargo test ui::modal::textarea`
Expected: PASS (10 tests).

- [ ] **Step 5: Commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add src/ui/modal/textarea.rs src/ui/modal/mod.rs
git commit -m "Modal: TextArea, a small soft-wrapping multi-line box"
```

---

### Task 4: `Modal::PromptTag` state and renderer

**Files:**
- Create: `src/ui/modal/prompt_tag.rs`
- Modify: `src/ui/modal/mod.rs` (`mod prompt_tag;`, `pub use`, `Modal::PromptTag` variant, render guard at line 222-232)
- Modify: `src/app/render/overlay.rs:12-80` (draw arm)
- Modify: `src/app/input/modal/mod.rs:57-131` (router arm — temporary `Modal::PromptTag(_) => {}` so the exhaustive match compiles; Task 5 replaces it)
- Test: inline `mod tests` in `src/ui/modal/prompt_tag.rs`

**Interfaces:**
- Consumes: `crate::commands::tags::{PromptTag, is_valid_name}`, `crate::ui::modal::TextArea`.
- Produces:
  ```rust
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub enum TagStage { Pick, Body { name: String } }
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct PromptTagModal { pub stage: TagStage, pub name_field: String, pub selected: usize, pub body: TextArea }
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub enum EnterAction { Existing(String), Create(String) }
  impl PromptTagModal {
      pub fn pick() -> Self;
      pub fn for_tag(name: &str) -> Self;
      pub fn filtered<'a>(&self, tags: &'a [PromptTag]) -> Vec<&'a PromptTag>;
      pub fn accelerators_active(&self) -> bool;     // name_field.is_empty()
      pub fn select_up(&mut self); pub fn select_down(&mut self, len: usize);
      pub fn enter_action(&self, tags: &[PromptTag]) -> Option<EnterAction>;
      pub fn name_invalid(&self) -> bool;            // non-empty and !is_valid_name
  }
  pub fn render_prompt_tag(f: &mut Frame, area: Rect, modal: &PromptTagModal, tags: &[PromptTag], theme: &Theme);
  ```
  `Modal::PromptTag(PromptTagModal)`.

- [ ] **Step 1: Write the failing tests**

Create `src/ui/modal/prompt_tag.rs` with the tests module:

```rust
//! The prompt-tag modal: pick (or name) an XML tag, then type the body
//! that gets wrapped in it and inserted into the focused agent's composer.
//! State and the pure list helpers live here; key handling is in
//! `app::input::modal::prompt_tag`.

#[cfg(test)]
mod tests {
    use super::*;

    fn tags() -> Vec<PromptTag> {
        ["context", "constraints", "task"]
            .iter()
            .enumerate()
            .map(|(i, n)| PromptTag {
                name: (*n).into(),
                uses: 9 - i as u32,
            })
            .collect()
    }

    #[test]
    fn pick_starts_on_the_first_tag_with_accelerators_on() {
        let m = PromptTagModal::pick();
        assert_eq!(m.stage, TagStage::Pick);
        assert_eq!(m.selected, 0);
        assert!(m.accelerators_active());
    }

    #[test]
    fn for_tag_opens_straight_into_the_body_stage() {
        let m = PromptTagModal::for_tag("context");
        assert_eq!(
            m.stage,
            TagStage::Body {
                name: "context".into()
            }
        );
        assert!(m.body.is_blank());
    }

    #[test]
    fn filtering_is_a_case_insensitive_prefix_match_that_disables_accelerators() {
        let all = tags();
        let mut m = PromptTagModal::pick();
        assert_eq!(m.filtered(&all).len(), 3);
        m.name_field = "CON".into();
        let names: Vec<_> = m.filtered(&all).iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["context", "constraints"]);
        assert!(!m.accelerators_active());
    }

    #[test]
    fn selection_clamps_to_the_filtered_list() {
        let mut m = PromptTagModal::pick();
        m.select_up();
        assert_eq!(m.selected, 0);
        m.select_down(3);
        m.select_down(3);
        m.select_down(3);
        assert_eq!(m.selected, 2);
        m.select_down(0);
        assert_eq!(m.selected, 0, "an empty list has nothing to select");
    }

    #[test]
    fn enter_picks_the_selection_or_creates_a_valid_new_name() {
        let all = tags();
        let mut m = PromptTagModal::pick();
        m.selected = 1;
        assert_eq!(
            m.enter_action(&all),
            Some(EnterAction::Existing("constraints".into()))
        );
        m.name_field = "task".into();
        m.selected = 0;
        assert_eq!(m.enter_action(&all), Some(EnterAction::Existing("task".into())));
        m.name_field = "examples".into();
        assert_eq!(m.enter_action(&all), Some(EnterAction::Create("examples".into())));
        m.name_field = "bad name".into();
        assert_eq!(m.enter_action(&all), None);
        assert!(m.name_invalid());
        m.name_field.clear();
        assert!(!m.name_invalid());
        assert_eq!(
            PromptTagModal::pick().enter_action(&[]),
            None,
            "nothing listed and nothing typed"
        );
    }
}
```

- [ ] **Step 2: Wire the variant so it compiles, run to see the tests fail**

In `src/ui/modal/mod.rs`:
- add `mod prompt_tag;` to the module list;
- add `pub use prompt_tag::{EnterAction, PromptTagModal, TagStage, render_prompt_tag};`;
- add the variant to `Modal`, after `RepoSettings`:
  ```rust
      /// Pick or name an XML tag and type the body to wrap in it; see
      /// `ui::modal::prompt_tag`. Rendered from `draw()` because it lists
      /// the live tag cache.
      PromptTag(PromptTagModal),
  ```
- add `| Modal::PromptTag(..)` to the guard `matches!` in `render` (line 222-232).

In `src/app/input/modal/mod.rs` add a temporary arm before the closing brace of the match: `Modal::PromptTag(_) => {}` (Task 5 replaces it).

Run: `cargo test ui::modal::prompt_tag`
Expected: compile errors — `PromptTagModal` etc. undefined.

- [ ] **Step 3: Implement the state, helpers and renderer**

Above the tests module in `src/ui/modal/prompt_tag.rs`:

```rust
use super::{TextArea, panel_frame};
use crate::commands::tags::{PromptTag, is_valid_name};
use crate::ui::theme::Theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagStage {
    /// Choosing (or naming) the tag.
    Pick,
    /// Typing the body for `name`.
    Body { name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptTagModal {
    pub stage: TagStage,
    /// The pick stage's text field: filters the list and names a new tag.
    pub name_field: String,
    /// Index into the FILTERED list.
    pub selected: usize,
    /// The body draft. Survives Esc from the body stage back to pick.
    pub body: TextArea,
}

/// What Enter in the pick stage does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnterAction {
    Existing(String),
    Create(String),
}

impl PromptTagModal {
    pub fn pick() -> Self {
        Self {
            stage: TagStage::Pick,
            name_field: String::new(),
            selected: 0,
            body: TextArea::new(),
        }
    }

    pub fn for_tag(name: &str) -> Self {
        Self {
            stage: TagStage::Body {
                name: name.to_string(),
            },
            ..Self::pick()
        }
    }

    /// Tags whose name starts with the field's text, case-insensitively,
    /// in the caller's (display) order.
    pub fn filtered<'a>(&self, tags: &'a [PromptTag]) -> Vec<&'a PromptTag> {
        let needle = self.name_field.to_ascii_lowercase();
        tags.iter()
            .filter(|t| t.name.to_ascii_lowercase().starts_with(&needle))
            .collect()
    }

    /// `1`–`9` jump straight to a listed tag only while nothing is typed;
    /// once the field has text a digit is part of a name.
    pub fn accelerators_active(&self) -> bool {
        self.name_field.is_empty()
    }

    pub fn select_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn select_down(&mut self, len: usize) {
        self.selected = (self.selected + 1).min(len.saturating_sub(1));
    }

    /// The typed name is non-empty and not a legal tag name.
    pub fn name_invalid(&self) -> bool {
        !self.name_field.is_empty() && !is_valid_name(&self.name_field)
    }

    pub fn enter_action(&self, tags: &[PromptTag]) -> Option<EnterAction> {
        let list = self.filtered(tags);
        if let Some(t) = list.get(self.selected) {
            return Some(EnterAction::Existing(t.name.clone()));
        }
        if self.name_field.is_empty() || self.name_invalid() {
            return None;
        }
        Some(EnterAction::Create(self.name_field.clone()))
    }
}

pub fn render_prompt_tag(
    f: &mut Frame,
    area: Rect,
    modal: &PromptTagModal,
    tags: &[PromptTag],
    theme: &Theme,
) {
    let w = area.width.clamp(50, 90);
    let h = area.height.clamp(12, 24);
    match &modal.stage {
        TagStage::Pick => render_pick(f, area, w, h, modal, tags, theme),
        TagStage::Body { name } => render_body(f, area, w, h, modal, name, theme),
    }
}

fn render_pick(
    f: &mut Frame,
    area: Rect,
    w: u16,
    h: u16,
    modal: &PromptTagModal,
    tags: &[PromptTag],
    theme: &Theme,
) {
    let inner = panel_frame(f, area, w, h, " Prompt tag ", theme);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);
    let (field_area, list_area, footer_area) = (chunks[0], chunks[2], chunks[3]);

    let field_style = if modal.name_invalid() {
        theme.err_style()
    } else {
        Style::default()
    };
    let prompt = "  name: ";
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(prompt, theme.dim_style()),
            Span::styled(modal.name_field.clone(), field_style),
        ])),
        field_area,
    );
    f.set_cursor_position((
        field_area.x + (prompt.len() + modal.name_field.chars().count()) as u16,
        field_area.y,
    ));

    let list = modal.filtered(tags);
    let height = list_area.height as usize;
    let top = modal.selected.saturating_sub(height.saturating_sub(1));
    let mut lines: Vec<Line> = Vec::new();
    for (i, t) in list.iter().enumerate().skip(top).take(height) {
        let key = if modal.accelerators_active() && i < 9 {
            format!(" {} ", i + 1)
        } else {
            "   ".to_string()
        };
        let row = format!("  {key} {:<24} ×{}", t.name, t.uses);
        let style = if i == modal.selected {
            theme.selected_style()
        } else {
            Style::default()
        };
        lines.push(Line::from(Span::styled(row, style)));
    }
    if list.is_empty() {
        let hint = if modal.name_field.is_empty() {
            "  no saved tags yet — type a name and press enter"
        } else if modal.name_invalid() {
            "  letters, digits, _ . - only; must start with a letter or _"
        } else {
            "  enter creates this tag"
        };
        lines.push(Line::from(Span::styled(hint, theme.dim_style())));
    }
    f.render_widget(Paragraph::new(lines), list_area);

    f.render_widget(
        Paragraph::new("[\u{2191}/\u{2193}] move   [enter] body   [1-9] pick   [^d] delete   [esc] close")
            .style(theme.dim_style()),
        footer_area,
    );
}

fn render_body(
    f: &mut Frame,
    area: Rect,
    w: u16,
    h: u16,
    modal: &PromptTagModal,
    name: &str,
    theme: &Theme,
) {
    let inner = panel_frame(f, area, w, h, format!(" <{name}> "), theme);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);
    let box_area = Rect {
        x: chunks[0].x + 1,
        width: chunks[0].width.saturating_sub(2),
        ..chunks[0]
    };
    modal.body.render(f, box_area, theme);
    f.render_widget(
        Paragraph::new("[^s] insert   [enter] newline   [esc] back").style(theme.dim_style()),
        chunks[1],
    );
}
```

`panel_frame` is private to `ui::modal`; `prompt_tag` is a child module so `super::panel_frame` resolves. `Theme::err_style`, `dim_style`, `selected_style` exist (`src/ui/theme.rs:340-383`).

In `src/app/render/overlay.rs`, add an arm in `draw_modal` after the `RepoSettings` arm:

```rust
        crate::ui::modal::Modal::PromptTag(modal) => {
            let tags = crate::commands::tags::load(&app.store).unwrap_or_default();
            crate::ui::modal::render_prompt_tag(f, area, modal, &tags, &app.theme);
        }
```

(`load` reads the memoized setting — no SQLite round trip per frame after the first.)

- [ ] **Step 4: Run the tests**

Run: `cargo test ui::modal::prompt_tag && cargo build`
Expected: 5 tests PASS; the crate builds with the placeholder router arm.

- [ ] **Step 5: Commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add src/ui/modal/prompt_tag.rs src/ui/modal/mod.rs src/app/render/overlay.rs src/app/input/modal/mod.rs
git commit -m "Modal: PromptTag state and renderer (pick and body stages)"
```

---

### Task 5: Prompt-tag modal key handling

**Files:**
- Create: `src/app/input/modal/prompt_tag.rs`
- Modify: `src/app/input/modal/mod.rs` (module list, doc, router arm)
- Modify: `src/app/input/tests/mod.rs:30-40` (`mod prompt_tag;`)
- Test: `src/app/input/tests/prompt_tag.rs`

**Interfaces:**
- Consumes: `PromptTagModal`, `TagStage`, `EnterAction` (Task 4); `tags::{load, save, bump, remove, wrap}` (Task 1); `Session::insert_text` (Task 2); `active_session` (`src/app/input/attached.rs:16`).
- Produces: `pub(super) async fn prompt_tag(app: &mut App, _shared: &SharedApp, k: KeyEvent, modal: PromptTagModal) -> Result<()>`.

- [ ] **Step 1: Write the failing tests**

Create `src/app/input/tests/prompt_tag.rs`:

```rust
//! The prompt-tag modal: pick → body → insert, and the manager keys.

use super::*;
use super::common::*;
use crate::commands::tags::{self, PromptTag};
use crate::data::store::Store;
use crate::ui::modal::{Modal, PromptTagModal, TagStage};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

async fn type_str(app: &mut App, shared: &SharedApp, s: &str) {
    for c in s.chars() {
        handle_key_modal(app, shared, key(KeyCode::Char(c))).await.unwrap();
    }
}

fn modal(app: &App) -> PromptTagModal {
    match &app.modal {
        Some(Modal::PromptTag(m)) => m.clone(),
        other => panic!("expected the prompt-tag modal, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pick_enter_on_a_new_name_creates_it_and_opens_the_body() {
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let shared = shared_app();
    spawn_attached_workspace(&mut app);
    app.modal = Some(Modal::PromptTag(PromptTagModal::pick()));
    type_str(&mut app, &shared, "context").await;
    handle_key_modal(&mut app, &shared, key(KeyCode::Enter)).await.unwrap();
    assert_eq!(
        modal(&app).stage,
        TagStage::Body {
            name: "context".into()
        }
    );
    assert_eq!(
        tags::load(&app.store).unwrap(),
        vec![PromptTag {
            name: "context".into(),
            uses: 0
        }],
        "a created tag is persisted at zero uses"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pick_enter_on_an_invalid_name_does_nothing() {
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let shared = shared_app();
    spawn_attached_workspace(&mut app);
    app.modal = Some(Modal::PromptTag(PromptTagModal::pick()));
    type_str(&mut app, &shared, "1bad").await;
    handle_key_modal(&mut app, &shared, key(KeyCode::Enter)).await.unwrap();
    assert_eq!(modal(&app).stage, TagStage::Pick);
    assert!(tags::load(&app.store).unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pick_digit_jumps_to_the_nth_tag_only_while_the_field_is_empty() {
    let store = Store::open_in_memory().unwrap();
    tags::save(
        &store,
        &[
            PromptTag { name: "context".into(), uses: 5 },
            PromptTag { name: "task".into(), uses: 2 },
        ],
    )
    .unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let shared = shared_app();
    spawn_attached_workspace(&mut app);
    app.modal = Some(Modal::PromptTag(PromptTagModal::pick()));
    handle_key_modal(&mut app, &shared, key(KeyCode::Char('2'))).await.unwrap();
    assert_eq!(modal(&app).stage, TagStage::Body { name: "task".into() });

    app.modal = Some(Modal::PromptTag(PromptTagModal::pick()));
    type_str(&mut app, &shared, "t2").await;
    assert_eq!(modal(&app).stage, TagStage::Pick);
    assert_eq!(modal(&app).name_field, "t2", "digits are text once the field has content");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ctrl_d_deletes_the_selected_tag_and_persists() {
    let store = Store::open_in_memory().unwrap();
    tags::save(
        &store,
        &[
            PromptTag { name: "context".into(), uses: 5 },
            PromptTag { name: "task".into(), uses: 2 },
        ],
    )
    .unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let shared = shared_app();
    spawn_attached_workspace(&mut app);
    app.modal = Some(Modal::PromptTag(PromptTagModal::pick()));
    handle_key_modal(&mut app, &shared, key(KeyCode::Down)).await.unwrap();
    handle_key_modal(&mut app, &shared, ctrl('d')).await.unwrap();
    assert_eq!(
        tags::load(&app.store).unwrap(),
        vec![PromptTag { name: "context".into(), uses: 5 }]
    );
    assert_eq!(modal(&app).selected, 0, "selection clamps after the delete");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn body_ctrl_s_inserts_the_wrapped_text_bumps_the_count_and_closes() {
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let shared = shared_app();
    let ws_id = spawn_attached_workspace(&mut app);
    app.modal = Some(Modal::PromptTag(PromptTagModal::for_tag("context")));
    type_str(&mut app, &shared, "line one").await;
    handle_key_modal(&mut app, &shared, key(KeyCode::Enter)).await.unwrap();
    type_str(&mut app, &shared, "line two").await;
    handle_key_modal(&mut app, &shared, ctrl('s')).await.unwrap();
    assert!(app.modal.is_none(), "insert closes the modal");
    assert_eq!(
        tags::load(&app.store).unwrap(),
        vec![PromptTag { name: "context".into(), uses: 1 }]
    );

    // The session is `cat`, which echoes what it receives; the paste
    // markers are CSI sequences vt100 swallows, leaving the tag lines.
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    let session = app.sessions.get(test_primary_instance(&app, ws_id)).unwrap();
    let screen = session.parser.lock().unwrap().screen().contents();
    assert!(screen.contains("<context>"), "{screen:?}");
    assert!(screen.contains("line one"), "{screen:?}");
    assert!(screen.contains("line two"), "{screen:?}");
    assert!(screen.contains("</context>"), "{screen:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn body_ctrl_s_with_a_blank_body_stays_open_and_records_nothing() {
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let shared = shared_app();
    spawn_attached_workspace(&mut app);
    app.modal = Some(Modal::PromptTag(PromptTagModal::for_tag("context")));
    type_str(&mut app, &shared, "   ").await;
    handle_key_modal(&mut app, &shared, ctrl('s')).await.unwrap();
    assert!(matches!(app.modal, Some(Modal::PromptTag(_))));
    assert!(tags::load(&app.store).unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn body_esc_returns_to_pick_keeping_the_draft() {
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let shared = shared_app();
    spawn_attached_workspace(&mut app);
    app.modal = Some(Modal::PromptTag(PromptTagModal::for_tag("context")));
    type_str(&mut app, &shared, "draft").await;
    handle_key_modal(&mut app, &shared, key(KeyCode::Esc)).await.unwrap();
    let m = modal(&app);
    assert_eq!(m.stage, TagStage::Pick);
    assert_eq!(m.body.text(), "draft");
    handle_key_modal(&mut app, &shared, key(KeyCode::Esc)).await.unwrap();
    assert!(app.modal.is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn body_ctrl_s_without_a_session_reports_an_error() {
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let shared = shared_app();
    // Dashboard view: no focused pane to insert into.
    app.modal = Some(Modal::PromptTag(PromptTagModal::for_tag("context")));
    type_str(&mut app, &shared, "x").await;
    handle_key_modal(&mut app, &shared, ctrl('s')).await.unwrap();
    assert!(
        matches!(&app.modal, Some(Modal::Error { message }) if message.contains("no running agent")),
        "{:?}",
        app.modal
    );
    assert!(tags::load(&app.store).unwrap().is_empty());
}
```

Register it: add `mod prompt_tag;` to the list in `src/app/input/tests/mod.rs` (alphabetical, after `mod process_command;`). `test_primary_instance` comes through `super::*` like the leader tests use it; if it does not resolve, import it from wherever `src/app/input/tests/leader.rs` gets it (`grep -rn "fn test_primary_instance" src/`).

- [ ] **Step 2: Run to see them fail**

Run: `cargo test input::tests::prompt_tag`
Expected: all 8 FAIL (the placeholder arm ignores every key: stages never change, nothing persists).

- [ ] **Step 3: Implement the handler**

Create `src/app/input/modal/prompt_tag.rs`:

```rust
//! Keys for the prompt-tag modal (`ui::modal::prompt_tag`): the pick
//! stage's list/filter/manager keys and the body stage's text box, ending
//! in an unsubmitted insert into the focused pane.

use super::*;
use crate::app::{App, SharedApp};
use crate::commands::tags;
use crate::error::Result;
use crate::ui::View;
use crate::ui::modal::{EnterAction, Modal, PromptTagModal, TagStage};
use crossterm::event::{KeyCode, KeyModifiers};

/// Pick-stage keys. Returns `true` when the modal was closed (the caller
/// must not put it back).
fn pick_key(app: &mut App, k: crossterm::event::KeyEvent, ctrl: bool, modal: &mut PromptTagModal) -> bool {
    let mut list = match tags::load(&app.store) {
        Ok(list) => list,
        Err(e) => {
            // Editing on top of an unreadable list could wipe it on save.
            app.modal = Some(Modal::Error {
                message: format!("could not read prompt tags: {e}"),
            });
            return true;
        }
    };
    match k.code {
        KeyCode::Esc => {
            app.modal = None;
            return true;
        }
        KeyCode::Up => modal.select_up(),
        KeyCode::Down => modal.select_down(modal.filtered(&list).len()),
        KeyCode::Char(c @ '1'..='9') if modal.accelerators_active() && !ctrl => {
            let idx = (c as u8 - b'1') as usize;
            if let Some(t) = modal.filtered(&list).get(idx) {
                modal.stage = TagStage::Body {
                    name: t.name.clone(),
                };
            }
        }
        KeyCode::Char('d') if ctrl => {
            let name = modal
                .filtered(&list)
                .get(modal.selected)
                .map(|t| t.name.clone());
            if let Some(name) = name {
                tags::remove(&mut list, &name);
                persist(app, &list);
                let len = modal.filtered(&list).len();
                modal.selected = modal.selected.min(len.saturating_sub(1));
            }
        }
        KeyCode::Enter => match modal.enter_action(&list) {
            Some(EnterAction::Existing(name)) => modal.stage = TagStage::Body { name },
            Some(EnterAction::Create(name)) => {
                list.push(tags::PromptTag {
                    name: name.clone(),
                    uses: 0,
                });
                tags::sort(&mut list);
                persist(app, &list);
                modal.stage = TagStage::Body { name };
            }
            None => {}
        },
        KeyCode::Backspace => {
            modal.name_field.pop();
            modal.selected = 0;
        }
        KeyCode::Char(c) if !ctrl => {
            modal.name_field.push(c);
            modal.selected = 0;
        }
        _ => {}
    }
    false
}

/// Body-stage keys. Returns `true` when the modal was replaced or closed
/// (insert succeeded, or an error modal took its place).
async fn body_key(
    app: &mut App,
    k: crossterm::event::KeyEvent,
    ctrl: bool,
    name: &str,
    modal: &mut PromptTagModal,
) -> bool {
    match k.code {
        KeyCode::Esc => {
            modal.stage = TagStage::Pick;
        }
        KeyCode::Char('s') if ctrl => {
            if modal.body.is_blank() {
                return false;
            }
            // Only a local or remote attached pane has a composer to insert
            // into; the dashboard's digest pane is not a PTY.
            let session = match app.view {
                View::Attached(_) | View::AttachedRemote => active_session(app),
                View::Dashboard => None,
            };
            let Some(session) = session else {
                app.modal = Some(Modal::Error {
                    message: "no running agent in the focused pane".to_string(),
                });
                return true;
            };
            let text = tags::wrap(name, &modal.body.text());
            if !session.insert_text(&text).await {
                app.modal = Some(Modal::Error {
                    message: "agent is not running".to_string(),
                });
                return true;
            }
            let mut list = match tags::load(&app.store) {
                Ok(list) => list,
                Err(e) => {
                    // The text is already in the composer; only the count
                    // is lost. Say so rather than saving over an unread list.
                    app.modal = Some(Modal::Error {
                        message: format!("inserted, but could not read prompt tags to count the use: {e}"),
                    });
                    return true;
                }
            };
            tags::bump(&mut list, name);
            app.modal = None;
            persist(app, &list);
            return true;
        }
        KeyCode::Enter => modal.body.newline(),
        KeyCode::Backspace => modal.body.backspace(),
        KeyCode::Delete => modal.body.delete(),
        KeyCode::Left => modal.body.move_left(),
        KeyCode::Right => modal.body.move_right(),
        KeyCode::Up => modal.body.move_up(),
        KeyCode::Down => modal.body.move_down(),
        KeyCode::Home => modal.body.home(),
        KeyCode::End => modal.body.end(),
        KeyCode::Char(c) if !ctrl => modal.body.insert_char(c),
        _ => {}
    }
    false
}

/// Write the list back. A failed write surfaces as an error modal (which
/// replaces the prompt-tag modal — the in-memory edit is lost, but the user
/// sees why) rather than silently reading as saved.
fn persist(app: &mut App, list: &[tags::PromptTag]) {
    if let Err(e) = tags::save(&app.store, list) {
        tracing::warn!(error = %e, "failed to persist prompt tags");
        app.modal = Some(Modal::Error {
            message: format!("could not save prompt tags: {e}"),
        });
    }
}
```

The entry point puts the modal back unless a stage closed or replaced it:

```rust
pub(super) async fn prompt_tag(
    app: &mut App,
    _shared: &SharedApp,
    k: crossterm::event::KeyEvent,
    mut modal: PromptTagModal,
) -> Result<()> {
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    let closed = match modal.stage.clone() {
        TagStage::Pick => pick_key(app, k, ctrl, &mut modal),
        TagStage::Body { name } => body_key(app, k, ctrl, &name, &mut modal).await,
    };
    if !closed && !matches!(app.modal, Some(Modal::Error { .. })) {
        app.modal = Some(Modal::PromptTag(modal));
    }
    Ok(())
}
```

(The `Error` check keeps a `persist` failure's error modal on screen instead of overwriting it.)

In `src/app/input/modal/mod.rs`:
- add `pub(super) mod prompt_tag;` to the module list and a doc line `//!   [`prompt_tag`]  the XML prompt-tag picker and body box`;
- replace the placeholder arm with `Modal::PromptTag(modal) => prompt_tag::prompt_tag(app, shared, k, modal).await?,`.

`active_session` is `pub(in crate::app::input)` in `src/app/input/attached.rs`; it is reachable through `use super::*;` (the modal module glob-imports `app::input`). If not, import it explicitly: `use crate::app::input::attached::active_session;`.

- [ ] **Step 4: Run the tests**

Run: `cargo test input::tests::prompt_tag`
Expected: 8 PASS. If `body_ctrl_s_inserts_…` is flaky on the screen assertion, raise the sleep to 300 ms — the leader tests use the same echo-through-`cat` pattern.

- [ ] **Step 5: Commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add src/app/input/modal/prompt_tag.rs src/app/input/modal/mod.rs src/app/input/tests/prompt_tag.rs src/app/input/tests/mod.rs
git commit -m "Prompt tags: modal keys — pick, create, delete, body, insert"
```

---

### Task 6: `^x <` leader key and nav-overlay row

**Files:**
- Modify: `src/app/input/leader.rs:160-172` (new arm before the digit arm)
- Modify: `src/app/input/attached.rs:250-268` (remote leader branch)
- Modify: `src/ui/attached/nav_menu.rs:64-75` (row after `k processes`)
- Modify: `src/ui/attached/agents_row.rs:27-32` (comment only)
- Test: `src/app/input/tests/leader.rs`, `src/ui/attached/nav_menu.rs` tests

**Interfaces:**
- Consumes: `Modal::PromptTag`, `PromptTagModal::pick()`.

- [ ] **Step 1: Write the failing tests**

Append to `src/app/input/tests/leader.rs`:

```rust
/// `^x <` opens the prompt-tag modal in its pick stage and clears the leader.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn leader_less_than_opens_the_prompt_tag_modal() {
    use crossterm::event::{KeyCode, KeyEvent};
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let ws_id = spawn_attached_workspace(&mut app);
    let target = test_target(&app, ws_id);
    handle_key_attached(
        &mut app,
        target,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
    )
    .await
    .unwrap();
    handle_key_attached(
        &mut app,
        target,
        KeyEvent::new(KeyCode::Char('<'), KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(!app.leader_pending);
    assert!(
        matches!(
            &app.modal,
            Some(crate::ui::modal::Modal::PromptTag(m))
                if m.stage == crate::ui::modal::TagStage::Pick
        ),
        "{:?}",
        app.modal
    );
}
```

In `src/ui/attached/nav_menu.rs`'s tests module, extend the existing single-pane items test (the one asserting the `detach` row, around line 205) with:

```rust
        assert!(
            items.iter().any(|i| i.glyph == "<" && i.label == "prompt tag"),
            "the nav overlay lists the prompt-tag row"
        );
        assert_eq!(
            crate::ui::footer::key_for_glyph("<").map(|k| k.code),
            Some(crossterm::event::KeyCode::Char('<')),
            "the overlay's Enter path resolves the row to the `<` key"
        );
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test leader_less_than_opens && cargo test nav_menu`
Expected: the leader test fails (`app.modal` is `None`); the nav test fails on the `any(...)` assertion.

- [ ] **Step 3: Implement**

`src/app/input/leader.rs`, inside `dispatch_leader_action`'s match, directly before the `KeyCode::Char(c @ '1'..='9')` arm:

```rust
        KeyCode::Char('<') => {
            // Prompt tag: pick an XML tag, type a body, insert it unsubmitted.
            app.modal = Some(Modal::PromptTag(
                crate::ui::modal::PromptTagModal::pick(),
            ));
            Ok(())
        }
```

`src/app/input/attached.rs`, in `handle_key_attached_remote`'s `if app.leader_pending` block, after the `'d'` check and before the digit chord:

```rust
        if k.code == KeyCode::Char('<') {
            app.modal = Some(crate::ui::modal::Modal::PromptTag(
                crate::ui::modal::PromptTagModal::pick(),
            ));
            return Ok(());
        }
```

`src/ui/attached/nav_menu.rs`, in `nav_menu_items`, after the `k processes` item and before `x send literal ^x`:

```rust
        NavItem {
            glyph: "<",
            label: "prompt tag",
        },
```

`src/ui/attached/agents_row.rs:28-29` comment: change to "(d, x, u, a, e, t, v, g, c, k, and `<` for prompt tags) plus all digits".

- [ ] **Step 4: Run the tests**

Run: `cargo test leader && cargo test nav_menu`
Expected: PASS. Also run `cargo test attached` to be sure the overlay width tests (`nav_row_width`) still pass with the new row.

- [ ] **Step 5: Commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add src/app/input/leader.rs src/app/input/attached.rs src/ui/attached/nav_menu.rs src/ui/attached/agents_row.rs src/app/input/tests/leader.rs
git commit -m "Attached view: ^x < opens the prompt-tag modal; nav overlay row"
```

---

### Task 7: `$tags` footer segment with click routing

**Files:**
- Modify: `src/ui/bar/registry.rs:120-145` (new `SegmentDef`)
- Modify: `src/ui/bar/segment.rs:14-24` (`Hit::TagChip(usize)`, `Hit::TagsManager`)
- Modify: `src/ui/bar/providers.rs` (new `tags` provider + import)
- Modify: `src/ui/bar/bars.rs:179-200` (`AttachedInputs.tags`), `:255-270` (`put` in `attached_bars`)
- Modify: `src/ui/bar/default_theme.toml:49-50` (format), after `[pins]` at `:137` (new table)
- Modify: `src/config/theme_file.rs:368-372` (error message)
- Modify: `src/ui/attached/mod.rs:40-100` (`PanesDrawOutput` fields + `route_hits`), `:134` (`render_panes` param + `AttachedInputs`), tests `:364-431`, `:857`
- Modify: `src/app/render/attached.rs` (`AttachedData.tags`, `gather_local`, `gather_remote`, both `render_panes` call sites, both cache copies)
- Modify: `src/app/state.rs:96-108`, `:627-680` (`tag_chip_rects`, `tags_manager_rect`, `prompt_tags_cache`)
- Modify: `src/app/render/mod.rs:108-126` (frame-start clears)
- Modify: `src/app/input/mouse.rs:201-208` (click routing)
- Test: `src/ui/bar/tests.rs`, `src/ui/bar/providers.rs`, `src/config/theme_file.rs`, `src/app/input/tests/mouse.rs`

**Interfaces:**
- Consumes: `tags::{PromptTag, CHIP_COUNT, load}`, `PromptTagModal::{pick, for_tag}`.
- Produces: `providers::tags(cfg: &SegmentConfig, tags: &[PromptTag], resolver: &Resolver) -> Option<Segment>`; `Hit::TagChip(usize)`, `Hit::TagsManager`; `PanesDrawOutput { tag_chip_rects: Vec<(usize, Rect)>, tags_manager_rect: Option<Rect>, .. }`; `App { tag_chip_rects, tags_manager_rect, prompt_tags_cache: Vec<PromptTag> }`.

- [ ] **Step 1: Write the failing tests**

Provider test — add to the tests module at the bottom of `src/ui/bar/providers.rs`:

```rust
    #[test]
    fn tags_renders_at_most_three_chips_then_the_manager_chip() {
        use crate::commands::tags::PromptTag;
        let theme = Theme::wsx();
        let specs = crate::config::theme_file::bundled_default(&theme);
        let resolver = specs.resolver(&theme);
        let cfg = &specs.segments["tags"];
        let four: Vec<PromptTag> = ["context", "task", "constraints", "examples"]
            .iter()
            .map(|n| PromptTag {
                name: (*n).into(),
                uses: 1,
            })
            .collect();
        let seg = tags(cfg, &four, &resolver).unwrap();
        let text = crate::ui::bar::test_util::plain(&seg.line());
        assert_eq!(text, "<context>  <task>  <constraints>   <> ");
        let hits: Vec<_> = seg.hits.iter().map(|h| h.hit).collect();
        assert_eq!(
            hits,
            vec![Hit::TagChip(0), Hit::TagChip(1), Hit::TagChip(2), Hit::TagsManager]
        );
        let manager = seg.hits.iter().find(|h| h.hit == Hit::TagsManager).unwrap();
        assert_eq!(manager.width, 4, "` <> ` is the manager pill");

        // No tags at all still gives the manager chip, and nothing else.
        let seg = tags(cfg, &[], &resolver).unwrap();
        assert_eq!(crate::ui::bar::test_util::plain(&seg.line()), " <> ");
        assert_eq!(seg.hits.len(), 1);
        assert_eq!(seg.hits[0].hit, Hit::TagsManager);
    }
```

If `Segment` has no `line()` accessor, use whatever the neighbouring provider tests use to get plain text from a `Segment` (e.g. `plain(&Line::from(seg.spans.clone()))`) — copy from `pins`' tests at `providers.rs:940-965`.

Theme-file test — in `src/config/theme_file.rs` tests, after `more_format_takes_count_on_attention_and_nothing_elsewhere`:

```rust
    #[test]
    fn tags_takes_a_more_format_with_count() {
        let specs = ok("[tags]\nmore_format = \"[$count tags](fg:dim)\"\n");
        assert_eq!(
            specs.segments["tags"].more_format,
            format::parse("[$count tags](fg:dim)").unwrap()
        );
        assert!(!errs("[tags]\nmore_format = \"$label\"\n").is_empty());
    }
```

Bar composer tests in `src/ui/bar/tests.rs`:
- `attention_budget_tests::inputs` and the bottom-bar module's `full`: add `tags: &[],`.
- `default_bottom_snapshot_and_hits`: the manager chip now follows the pins. Change the `starts_with` expectation to `" ^x  menu   1  PR   2  feedback   <>   ──"` (pins group, two spaces, ` <> `, two spaces, rule). If the assertion's failure output shows a different whitespace count, the group spacing in `default_theme.toml` is the source of truth — match the output, and confirm the `<>` chip is separated from `feedback` by exactly the format's two spaces plus the pill's own pad.
- Add, in the bottom-bar module:

```rust
    #[test]
    fn tag_chips_follow_the_pins_and_carry_their_hits() {
        let pinned = cmds(&[("PR", "/pr")]);
        let agents = agents();
        let tags = vec![
            crate::commands::tags::PromptTag { name: "context".into(), uses: 3 },
            crate::commands::tags::PromptTag { name: "task".into(), uses: 1 },
        ];
        let mut inputs = full(&pinned, &agents);
        inputs.tags = &tags;
        let out = render(inputs, 140);
        let t = plain(&out.line);
        assert!(t.starts_with(" ^x  menu   1  PR  <context>  <task>   <>   ──"), "{t:?}");
        let chips: Vec<_> = out
            .hits
            .iter()
            .filter_map(|h| match h.hit {
                Hit::TagChip(i) => Some(i),
                _ => None,
            })
            .collect();
        assert_eq!(chips, vec![0, 1]);
        assert!(out.hits.iter().any(|h| h.hit == Hit::TagsManager));
    }
```

- `attached_segments_cover_every_registered_segment` (line 854): give the inputs a one-tag list so `tags` renders non-empty like the others.

Mouse test — append to `src/app/input/tests/mouse.rs`:

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn clicking_a_tag_chip_opens_the_body_stage_and_the_manager_chip_opens_pick() {
    use crate::commands::tags::PromptTag;
    use crate::ui::modal::{Modal, TagStage};
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    spawn_attached_workspace(&mut app);
    app.prompt_tags_cache = vec![PromptTag { name: "context".into(), uses: 2 }];
    app.tag_chip_rects = vec![(0, ratatui::layout::Rect::new(10, 23, 9, 1))];
    app.tags_manager_rect = Some(ratatui::layout::Rect::new(22, 23, 4, 1));

    let mut m = mouse_event(MouseEventKind::Down(MouseButton::Left));
    m.column = 12;
    m.row = 23;
    handle_mouse(&mut app, &shared_app(), m).await.unwrap();
    assert!(
        matches!(&app.modal, Some(Modal::PromptTag(x)) if x.stage == TagStage::Body { name: "context".into() }),
        "{:?}",
        app.modal
    );

    app.modal = None;
    let mut m = mouse_event(MouseEventKind::Down(MouseButton::Left));
    m.column = 23;
    m.row = 23;
    handle_mouse(&mut app, &shared_app(), m).await.unwrap();
    assert!(
        matches!(&app.modal, Some(Modal::PromptTag(x)) if x.stage == TagStage::Pick),
        "{:?}",
        app.modal
    );
}
```

Copy the exact `handle_mouse` call shape (arguments, imports for `Store`/`PathBuf`) from an existing test in that file, e.g. the chip-click one near the top.

- [ ] **Step 2: Run to see them fail**

Run: `cargo test tags_renders_at_most && cargo test tag_chips_follow && cargo test clicking_a_tag_chip`
Expected: compile errors (`tags` provider, `Hit::TagChip`, `AttachedInputs.tags`, `App.tag_chip_rects` all missing).

- [ ] **Step 3: Implement the segment**

`src/ui/bar/segment.rs`, in `enum Hit` after `PinnedChip(usize)`:

```rust
    /// A prompt-tag chip: index into the sorted tag cache
    /// (`App::prompt_tags_cache`), opening the body stage for that tag.
    TagChip(usize),
    /// The trailing `<>` chip: opens the prompt-tag picker.
    TagsManager,
```

`src/ui/bar/registry.rs`, after the `pins` def:

```rust
    SegmentDef {
        name: "tags",
        vars: &["index", "label"],
        style_vars: STYLE,
        // The manager chip is the tail; `$count` is how many tags exist.
        more_vars: &["count"],
        items: true,
        // One un-indexed target (the manager chip), like `attention`'s tail.
        singleton: true,
    },
```

`src/ui/bar/providers.rs`: add `use crate::commands::tags::{CHIP_COUNT, PromptTag};` to the imports and, after `pins`:

```rust
/// Prompt-tag chips: the first `CHIP_COUNT` tags (the caller passes them
/// most-used first), then the manager chip from `more_format`. The manager
/// chip always renders — with no tags it is the only way to discover the
/// feature from the footer — so this segment is never empty unless disabled.
pub fn tags(cfg: &SegmentConfig, tags: &[PromptTag], resolver: &Resolver) -> Option<Segment> {
    if cfg.disabled {
        return None;
    }
    let items: Vec<(SegmentMap, Style, Option<Hit>)> = tags
        .iter()
        .take(CHIP_COUNT)
        .enumerate()
        .map(|(i, t)| {
            (
                vars(vec![
                    ("index", var((i + 1).to_string())),
                    ("label", var(truncate_label(&t.name, CHIP_LABEL_COLS))),
                ]),
                Style::default(),
                Some(Hit::TagChip(i)),
            )
        })
        .collect();
    let mut out = eval_items(cfg, &items, resolver).unwrap_or_default();
    let resolver = &resolver.with_overlay(&cfg.palette);
    if !out.is_empty() {
        out.append(eval(&cfg.separator, &SegmentMap::new(), resolver, Style::default()).0);
    }
    let start = out.width;
    let v = vars(vec![("count", var(tags.len().to_string()))]);
    out.append(eval(&cfg.more_format, &v, resolver, Style::default()).0);
    out.hit_from(start, Hit::TagsManager);
    (!out.is_empty()).then_some(out)
}
```

`src/ui/bar/bars.rs`: add `pub tags: &'a [crate::commands::tags::PromptTag],` to `AttachedInputs` after `pinned`, and in `attached_bars` after the `pins` `put`:

```rust
    put(
        &mut segments,
        "tags",
        providers::tags(cfg(specs, "tags"), inputs.tags, resolver),
    );
```

`src/ui/bar/default_theme.toml`: change `[attached_bottom].format` to `"$keys  ($pins  )($tags  )"` and add after the `[pins]` table:

```toml
# Prompt tags: the three most-used tags as chips, then the manager chip
# (`more_format`; its `$count` is how many tags are saved). Clicking a chip
# opens the body box for that tag; the manager chip opens the picker
# (`^x <`). Variables: $index $label
[tags]
format      = "[<$label>](fg:path)"
separator   = "  "
more_format = "[ <> ](bg:bg_soft fg:dim bold)"
priority    = 40
```

`src/config/theme_file.rs:371`: change the message to "this segment has no overflow tail (only `attention` and `tags` take `more_format`)". The existing `[pins]` test only checks `contains("no overflow tail")`, so it keeps passing.

- [ ] **Step 4: Route the hits through the attached view and the app**

`src/ui/attached/mod.rs`:
- `PanesDrawOutput` gains
  ```rust
      /// `(tag index, clickable rect)` per prompt-tag chip; the index is into
      /// `App::prompt_tags_cache`, as `chip_rects` is into the pinned cache.
      pub tag_chip_rects: Vec<(usize, Rect)>,
      /// The prompt-tag manager chip, when a theme places `$tags`.
      pub tags_manager_rect: Option<Rect>,
  ```
- `route_hits` gains `Hit::TagChip(i) => out.tag_chip_rects.push((i, rect)),` and `Hit::TagsManager => out.tags_manager_rect = Some(rect),`.
- `render_panes` gains a parameter `tags: &[crate::commands::tags::PromptTag],` after `pinned`, forwarded as `tags,` in the `AttachedInputs` literal. Update every caller and the tests in this file (`bottom_fixture` users and the `pinned: &[]` fixtures at lines 364 and 857) to pass `&[]`.

`src/app/state.rs`: add fields next to their pinned counterparts, with initializers in `App::new`:

```rust
    /// `(tag index, rect)` per prompt-tag chip on the attached chip row.
    /// Mirrors the `chip_rects` draw-populates / input-reads pattern.
    pub tag_chip_rects: Vec<(usize, ratatui::layout::Rect)>,
    /// The prompt-tag manager chip's rect, when drawn.
    pub tags_manager_rect: Option<ratatui::layout::Rect>,
    /// Sorted prompt tags from the last draw tick (matches `tag_chip_rects`).
    pub prompt_tags_cache: Vec<crate::commands::tags::PromptTag>,
```

`src/app/render/mod.rs` frame-start block: add `app.tag_chip_rects.clear(); app.tags_manager_rect = None; app.prompt_tags_cache.clear();` beside the pinned clears.

`src/app/render/attached.rs`:
- `AttachedData` gains `tags: Vec<crate::commands::tags::PromptTag>,`; `inputs()` passes `tags: &self.tags,`.
- `gather_local` and `gather_remote` set `tags: crate::commands::tags::load(&app.store).unwrap_or_default(),` (display only — an unreadable list just shows no chips) (the remote view inserts into the ssh PTY exactly as pins do).
- Both `render_panes` call sites pass `&data.tags,` after `&data.pinned,`.
- Both cache-copy blocks add `app.tag_chip_rects = out.tag_chip_rects; app.tags_manager_rect = out.tags_manager_rect; app.prompt_tags_cache = data.tags;`.

`src/app/input/mouse.rs`, after the `chip_rects` → `fire_chip` branch (make the chain `else if`):

```rust
            } else if let Some((idx, _)) = app.tag_chip_rects.iter().copied().find(|(_, r)| {
                m.column >= r.x
                    && m.column < r.x.saturating_add(r.width)
                    && m.row >= r.y
                    && m.row < r.y.saturating_add(r.height)
            }) {
                // A tag chip opens the body box for that tag straight away.
                if let Some(tag) = app.prompt_tags_cache.get(idx) {
                    app.modal = Some(Modal::PromptTag(
                        crate::ui::modal::PromptTagModal::for_tag(&tag.name),
                    ));
                }
            } else if app.tags_manager_rect.is_some_and(|r| {
                m.column >= r.x
                    && m.column < r.x.saturating_add(r.width)
                    && m.row >= r.y
                    && m.row < r.y.saturating_add(r.height)
            }) {
                // The manager chip is `^x <`.
                app.modal = Some(Modal::PromptTag(
                    crate::ui::modal::PromptTagModal::pick(),
                ));
```

- [ ] **Step 5: Run the full suite and fix snapshot fallout**

Run: `cargo test`
Expected: the new tests pass; some bottom-bar snapshot tests fail only because ` <> ` now occupies 6 more cells (4 for the pill, 2 for the group gap). For each such failure (`narrow_rows_drop_…`, `single_agent_stats_block_…`, `no_pr_leaves_only_…`, the attached `mod.rs` routing tests), update the expected width/column by exactly that amount or widen the fixture terminal; do not change the assertion's intent. The `dashboard_detail` tests must be untouched — `$tags` is not registered there.

Run: `cargo run -- theme check` (or the equivalent `wsx theme check` on the bundled default) to confirm the default theme validates with the new table.

- [ ] **Step 6: Commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add src/ui/bar src/config/theme_file.rs src/ui/attached/mod.rs src/app/render src/app/state.rs src/app/input/mouse.rs src/app/input/tests/mouse.rs
git commit -m "Chat footer: \$tags segment — top-three prompt tags and the manager chip"
```

---

### Task 8: Docs, example themes, manual test, spec touch-up

**Files:**
- Modify: `README.md:36`
- Create: `docs/book/src/daily-use/prompt-tags.md`
- Modify: `docs/book/src/SUMMARY.md:11`
- Modify: `docs/book/src/configuration/themes.md:159`, `:319`, `:325-335`
- Modify: `docs/examples/theme-{nord,nord0,rose-pine,rose-pine-moon,orange,starship,jellybeans}.toml`
- Create: `docs/manual-tests/prompt-tags.md`
- Modify: `docs/superpowers/specs/2026-09-16-prompt-tags-design.md` (non-goals: remote view)

- [ ] **Step 1: Example themes must validate**

For each example theme, add `($tags  )` (or the theme's own block styling) immediately after its `$pins` group in `[attached_bottom].format`, and a `[tags]` table copying that theme's `[pins]` colours, e.g. for `theme-starship.toml`:

```toml
[tags]
format      = "[<$label>](fg:label)"
separator   = "  "
more_format = "[ <> ](fg:amber bold)"
priority    = 40
```

Verify: `for f in docs/examples/theme-*.toml; do cargo run -q -- theme check "$f" || echo "FAIL $f"; done` (use the actual `theme check` invocation from `docs/book/src/configuration/themes.md`).
Expected: every file validates.

- [ ] **Step 2: Book page**

Create `docs/book/src/daily-use/prompt-tags.md`:

````markdown
# Prompt tags

Anthropic's prompting guidance recommends separating the parts of a prompt
with XML tags — `<context>…</context>`, `<task>…</task>`,
`<constraints>…</constraints>` — so the model can tell them apart. Prompt
tags make that one keystroke from the attached view.

Press `Ctrl-x <` (or click the `<>` chip in the chat footer) to open the
picker:

```
 name: ▏
  1  context                   ×12
  2  task                      ×7
  3  constraints               ×2
 [↑/↓] move   [enter] body   [1-9] pick   [^d] delete   [esc] close
```

- Typing filters the list by prefix. `Enter` on a listed tag opens its
  body box; `Enter` on a name that isn't listed creates it.
- `1`–`9` jump straight to a listed tag while the name field is empty.
- `Ctrl-d` deletes the selected tag.

The body box is a small multi-line editor: `Enter` inserts a newline,
arrows/Home/End move, `Esc` goes back to the picker (keeping your draft).
`Ctrl-s` inserts

```
<context>
…your text…
</context>
```

into the agent's composer **without submitting**, so you can stack several
tagged sections and add a plain instruction before pressing Enter yourself.
Each insert bumps the tag's use count; the three most-used tags sit in the
footer as `<context>`-style chips — click one to go straight to its body box.

Tag names must start with a letter or `_` and contain only letters, digits,
`_`, `.` and `-`.

The list lives in the `prompt_tags` setting, one `name=uses` per line:

```bash
wsx config get prompt_tags
wsx config edit prompt_tags                 # opens $EDITOR on the current value
wsx config set prompt_tags "context=12
task=7"
wsx config set prompt_tags ""               # clear
```

The footer chips are the `$tags` bar segment — see
[Themes](../configuration/themes.md) to move, restyle, or drop them. The
`Ctrl-x <` chord works even when a theme omits the segment.
````

Add `  - [Prompt tags](daily-use/prompt-tags.md)` to `docs/book/src/SUMMARY.md` directly after the pinned-commands line.

- [ ] **Step 3: Themes reference and README**

`docs/book/src/configuration/themes.md`:
- line 159: `format       = "$keys  ($pins  )($tags  )"`.
- after the `pins` row in the segment table: `| \`tags\` | \`$index $label\` | The three most-used prompt tags as chips, then the manager chip from \`more_format\` (\`$count\` = saved tags). Attached only. Clickable: each chip, and the manager. |`
- the singleton paragraph: "`pr`, `procs`, `usage`, `attention`, and `tags` may each be placed only once … `attention` and `tags` … carry a single tail target …" — reword the sentence so it lists five names and explains that `tags`' tail is the manager chip.

`README.md:36`: extend the feature bullet to "…pinned commands, prompt tags (XML-wrapped prompt sections), and MCP…".

- [ ] **Step 4: Manual test page and the omp check**

Create `docs/manual-tests/prompt-tags.md`:

````markdown
# Manual test — Prompt tags

Spec: `docs/superpowers/specs/2026-09-16-prompt-tags-design.md`

Use a scratch state dir so the real tag list is untouched:

```bash
scratch=$(mktemp -d /tmp/wsx-tags-test.XXXXXX)
export XDG_CONFIG_HOME="$scratch/config" XDG_STATE_HOME="$scratch/state"
```

Register a repo and create one workspace per agent kind you have installed
(claude, codex, pi, hermes, omp). For each:

## 1. Insert lands unsubmitted, newlines intact

1. Attach, wait for the agent's composer.
2. `Ctrl-x <`, type `context`, Enter.
3. Type `first line`, Enter, `second line`, `Ctrl-s`.
4. Expect: the composer shows `<context>` / `first line` / `second line` /
   `</context>` (omp may show a `[Paste #1]` placeholder instead — that is
   expected) and the agent has NOT started a turn.
5. Type `summarise the above` and press Enter. Expect the agent's reply to
   reference both lines (this is the check that omp expands the placeholder).

Record the omp outcome in `insert_writes`' doc comment if it differs from
what the comment says.

## 2. Footer chips reorder by use

1. Insert under `context` twice and `task` once.
2. Expect the footer to read `<context>  <task>   <>` with `context` first.
3. Click `<task>`: the body box opens titled ` <task> `.
4. Click `<>`: the picker opens.

## 3. Manager keys

1. In the picker, type `Bad Name` — the field turns red and Enter does nothing.
2. Clear it, ↓ to `task`, `Ctrl-d` — `task` disappears;
   `wsx config get prompt_tags` no longer lists it.

## 4. Theme without `$tags`

1. `wsx theme init`, remove `($tags  )` from `[attached_bottom].format`.
2. Expect no chips, and `Ctrl-x <` still opens the picker.

## 5. Remote attach

1. Attach to a shared workspace on another host (`H`).
2. `Ctrl-x <`, pick a tag, insert. Expect the text in the remote agent's
   composer, unsubmitted.
````
Run section 1 against every installed agent now. If omp swallows the paste or submits on it, change `insert_writes`' `Omp` behaviour to plain text (`text.as_bytes().to_vec()`) and update its comment and the `insert_writes_wraps_every_agent…` test to exclude omp.

- [ ] **Step 5: Spec touch-up**

In `docs/superpowers/specs/2026-09-16-prompt-tags-design.md`, replace the non-goal "The dashboard detail chip row and the remote-attach view (`View::AttachedRemote`). The modal targets the focused local pane only." with "The dashboard detail chip row. (The remote-attach view does get the chips and `^x <`: its PTY is the ssh hop into the remote agent, so the insert is plain bytes exactly as `$pins` already behaves there.)"

- [ ] **Step 6: Build the book and commit**

Run: `cargo test && (cd docs/book && mdbook build)` if `mdbook` is installed; otherwise skip the book build and say so in the commit body.

```bash
git add README.md docs
git commit -m "Docs: prompt tags — book page, theme reference, example themes, manual test"
```

---

## Self-review

**Spec coverage:** data model/persistence → Task 1; byte shape → Task 2; text area → Task 3; modal state/render → Task 4; pick/body keys, insertion, error handling (invalid name, closed writer, no session, save failure) → Task 5; leader `<` + nav row → Task 6; `$tags` segment, hits, mouse, default theme, `priority = 40`, theme-file validation → Task 7; docs, example themes, manual test (incl. omp check) → Task 8. Spec's "footer refreshes after mutation" is satisfied by `tags::load` reading the memoized setting each frame (invalidated on `set_setting`), so no separate refresh step is needed.

**Placeholder scan:** none.

**Type consistency:** `PromptTag { name, uses }`, `tags::{load, save, bump, remove, sort, wrap, CHIP_COUNT}`, `PromptTagModal::{pick, for_tag, filtered, accelerators_active, select_up, select_down, enter_action, name_invalid}`, `TagStage::{Pick, Body { name }}`, `EnterAction::{Existing, Create}`, `TextArea` API, `Hit::{TagChip, TagsManager}`, `App::{tag_chip_rects, tags_manager_rect, prompt_tags_cache}`, `AttachedInputs.tags`, `providers::tags` are used with the same names and shapes in every task that references them.
