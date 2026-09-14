# Starship-style Bar Theming Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the dashboard footer and the attached view's top and bottom bars user-themeable through `~/.config/wsx/theme.toml` using a strict subset of starship's format grammar, with a bundled default that reproduces today's bars exactly.

**Architecture:** A new `src/ui/bar/` module holds a format-string parser, a style-string grammar, a `Segment` provider contract carrying click hits, and one evaluator that lays out `format`/`right_format` with priority-based overflow. `src/config/theme_file.rs` loads and validates the TOML (merged over a bundled default) into resolved `BarSpecs`. The three existing hand-written span builders are replaced one bar at a time, each gated by a parity test against the legacy output before the legacy code is deleted.

**Tech Stack:** Rust 2024 edition, ratatui 0.29, serde, `toml` 0.8 (new dependency), crossterm 0.29.

**Spec:** `docs/superpowers/specs/2026-09-13-bar-theming-design.md`

## Global Constraints

- Every task ends green under `cargo test`, `cargo clippy --all-targets -- -D warnings`, and `mise exec rust@1.95.0 -- cargo fmt --all --check` (CI pins rustfmt 1.95.0; the ambient rustfmt can false-pass).
- Do not edit files under `/home/eben/chronox` or `/home/eben/sessionx`.
- The bundled default must render every bar pixel-identical to today for the same inputs; parity tests prove this before legacy code is deleted. Whitespace cells compare background only (the new grammar puts fg/bold on pad cells; that is invisible on screen).
- No new background threads or file watchers. Reload is an mtime/length fingerprint check on the existing 125 ms tick, at most once per second.
- Commit after each task with the message given in the task. End every commit message with `Claude-Session: https://claude.ai/code/session_01MatH8nt8Q22cYv1yqQHUcg`.
- Style-string modifier for dim is spelled `dimmed` (the theme token `dim` is a color).
- The `theme` setting keeps its meaning (base palette). `theme.toml` layers on top; it never overrides built-in `Theme` fields.

## Segment vocabulary (reference for every task)

| Segment | Variables in its `format` | Style vars | Hit | Non-empty in |
|---|---|---|---|---|
| `keys` | `key`, `label` (one pill) | `style` | `Key`/`ArmLeader` per pill | both |
| `version` | `version` | `style` | — | both |
| `usage` | `label`, `spark` | `style` | `UsageGraph` | both |
| `agent_bar` | `symbol` | `style` | — | attached |
| `workspace` | `repo`, `name` | `style` | — | attached |
| `attention` | `items` | `style` | `Attention(ws)`, `AttentionMore` | attached |
| `pins` | `index`, `label` (one chip) | `style` | `PinnedChip(i)` | attached |
| `agents` | `symbol`, `label`, `key` (one pill) | `style` | `Agent(id)` | attached |
| `model_tokens` | `model`, `tokens` | `style` | — | attached |
| `procs` | `symbol`, `count` | `style` | `Procs` | attached |
| `diff` | `added`, `removed` | `style` | — | attached |
| `pr` | `symbol`, `number`, `label`, `mark` | `style`, `mark_style` | `Pr` | attached |

Deviations from the spec table, decided while reading the code: `model_tokens` exposes the pre-formatted `tokens` string (`45k/200k`) rather than `used`/`window`, because that is what `ChipModelTokens` carries; `pr` folds the unresolved count into `mark` (that is how `PrChip::mark` composes it); `agents` uses `label` (the instance label) rather than `kind`; `attention` exposes one opaque `items` variable. Update the spec's table in Task 11.

## File map

| File | Responsibility |
|---|---|
| `Cargo.toml` | add `toml = "0.8"` |
| `src/config/mod.rs` | `Dirs::config_dir()`, `Dirs::theme_path()` |
| `src/config/theme_file.rs` (new) | serde model of the TOML, merge over bundled default, validation, `BarSpecs`, `load`/`bundled_default` |
| `src/ui/bar/mod.rs` (new) | module root; the three bar composers `dashboard_footer`, `attached_top`, `attached_bottom` |
| `src/ui/bar/format.rs` (new) | format-string parser → `Node` AST; `vars()`, `styles()` walkers |
| `src/ui/bar/style.rs` (new) | `StyleSpec` parser, `ColorRef`, `Resolver` |
| `src/ui/bar/segment.rs` (new) | `Hit`, `HitSpan`, `Segment`, `SegmentConfig`, `SegmentMap` |
| `src/ui/bar/render.rs` (new) | `eval`, `render_bar`, `hit_rects`, `BarSpec`, `Rendered` |
| `src/ui/bar/providers.rs` (new) | one provider fn per segment |
| `src/ui/bar/default_theme.toml` (new) | bundled default, embedded with `include_str!` |
| `src/ui/bar/test_util.rs` (new, cfg(test)) | `assert_lines_match` cell comparator |
| `src/ui/theme.rs` | `Theme::token(name)` |
| `src/cli/{action,groups,parse/mod,parse/theme,run,tests}.rs` | `wsx theme check|path|init` |
| `src/ui/dashboard/{mod,layout}.rs`, `src/app/render/dashboard.rs` | footer on the engine; legacy `layout::footer` deleted |
| `src/ui/attached/{mod,chip_row,agents_row}.rs`, `src/app/render/attached.rs` | top and bottom bars on the engine; legacy builders deleted |
| `src/app/{state,run}.rs`, `src/main.rs` | `bar_specs`, theme path, reload, notice |
| `docs/book/src/configuration/themes.md`, `docs/manual-tests/bar-theming.md` | docs |

---

### Task 1: `toml` dependency and the theme path

**Files:**
- Modify: `Cargo.toml` (dependencies block)
- Modify: `src/config/mod.rs`

**Interfaces:**
- Produces: `Dirs::config_dir(&self) -> PathBuf` (`<config_root>/wsx`), `Dirs::theme_path(&self) -> PathBuf` (`<config_root>/wsx/theme.toml`). `Dirs::for_test(root)` puts the config root at `<root>/config`.

- [ ] **Step 1: Add the dependency**

Run: `cargo add toml@0.8`
Expected: `Cargo.toml` gains `toml = "0.8"` under `[dependencies]` and `Cargo.lock` updates. Run `cargo build` once to confirm it resolves.

- [ ] **Step 2: Write the failing test**

Append inside the existing `mod tests` in `src/config/mod.rs`:

```rust
    #[test]
    fn theme_path_under_config_dir() {
        let dirs = Dirs::for_test("/tmp/wsx-test-home");
        assert_eq!(
            dirs.config_dir(),
            std::path::PathBuf::from("/tmp/wsx-test-home/config/wsx")
        );
        assert_eq!(
            dirs.theme_path(),
            std::path::PathBuf::from("/tmp/wsx-test-home/config/wsx/theme.toml")
        );
    }
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test --lib config::tests::theme_path_under_config_dir`
Expected: compile error, `no method named config_dir`.

- [ ] **Step 4: Implement**

Replace the `Dirs` struct, `discover`, and `for_test` in `src/config/mod.rs` with:

```rust
#[derive(Clone, Debug)]
pub struct Dirs {
    state_root: PathBuf,
    /// Parent of the `wsx/` config directory: `$XDG_CONFIG_HOME` when set
    /// and absolute, else `~/.config`. Same on every platform, like
    /// starship, so `theme.toml` lives in one predictable place.
    config_root: PathBuf,
}

impl Dirs {
    pub fn discover() -> Self {
        // Honor an explicit `XDG_STATE_HOME` on every platform. `dirs::state_dir()`
        // only reads it on Linux; on macOS it returns `None`, which sent a sandboxed
        // run (sandbox/bootstrap.sh) straight into the real `~/.local/state` db.
        let state_root = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(dirs::state_dir)
            .or_else(|| dirs::home_dir().map(|h| h.join(".local/state")))
            .unwrap_or_else(|| PathBuf::from("."));
        let config_root = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
            .unwrap_or_else(|| PathBuf::from("."));
        Self {
            state_root,
            config_root,
        }
    }

    #[cfg(test)]
    pub fn for_test(root: impl AsRef<Path>) -> Self {
        Self {
            state_root: root.as_ref().to_path_buf(),
            config_root: root.as_ref().join("config"),
        }
    }

    /// `<config_root>/wsx` — user-editable config files (currently just
    /// `theme.toml`). Distinct from `app_dir`, which holds state (db, logs).
    pub fn config_dir(&self) -> PathBuf {
        self.config_root.join("wsx")
    }
    /// `~/.config/wsx/theme.toml` — the bar theme file.
    pub fn theme_path(&self) -> PathBuf {
        self.config_dir().join("theme.toml")
    }
```

Keep the existing `app_dir`, `db_path`, `log_dir`, `context_dir`, `ensure` methods unchanged.

- [ ] **Step 5: Run tests**

Run: `cargo test --lib config::tests`
Expected: both tests PASS.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/config/mod.rs
git commit -m "Config: theme.toml path under ~/.config/wsx, add toml dependency"
```

---

### Task 2: Format-string parser (`src/ui/bar/format.rs`)

**Files:**
- Create: `src/ui/bar/mod.rs`
- Create: `src/ui/bar/format.rs`
- Create: `src/ui/bar/style.rs` (stub only; filled in Task 3)
- Modify: `src/ui/mod.rs` (add `pub mod bar;`)

**Interfaces:**
- Produces:
  - `pub enum Node { Text(String), Var(String), Styled(Vec<Node>, StyleSpec), Group(Vec<Node>) }`
  - `pub struct ParseError { pub offset: usize, pub message: String }` (Display: `col {offset}: {message}`; offset is a 0-based char index)
  - `pub fn parse(src: &str) -> Result<Vec<Node>, ParseError>`
  - `pub fn vars(nodes: &[Node]) -> Vec<&str>` (every `$name`, depth-first, duplicates kept)
  - `pub fn styles(nodes: &[Node]) -> Vec<&StyleSpec>` (every `Styled` spec, depth-first)
- Consumes: `StyleSpec::parse(&str) -> Result<StyleSpec, StyleError>` from Task 3 (stubbed here).

- [ ] **Step 1: Create the module skeleton**

`src/ui/bar/mod.rs`:

```rust
//! The themeable bar engine: a starship-style format grammar, segment
//! providers that carry click hits, and one evaluator shared by the
//! dashboard footer and the attached view's top and bottom bars.
//!
//! See `docs/superpowers/specs/2026-09-13-bar-theming-design.md`.

pub mod format;
pub mod style;
```

`src/ui/bar/style.rs` stub (replaced in Task 3):

```rust
//! Style-string grammar: `fg:<c> bg:<c> <c> bold dimmed italic underline none $var`.

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StyleSpec {
    pub raw: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleError(pub String);

impl std::fmt::Display for StyleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl StyleSpec {
    pub fn parse(src: &str) -> Result<Self, StyleError> {
        Ok(Self {
            raw: src.to_string(),
        })
    }
}
```

Add `pub mod bar;` to `src/ui/mod.rs` (alphabetically, after `attached`).

- [ ] **Step 2: Write the failing tests**

`src/ui/bar/format.rs` with only the tests module for now:

```rust
//! Parser for the bar format mini-language, a strict subset of starship's:
//! `$name` / `${name}` inserts a segment, `[text](style)` styles a run,
//! `( … )` renders only when a `$name` inside produced output. `$$` is a
//! literal dollar; `\x` escapes any single character.

use super::style::StyleSpec;

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Node {
        Node::Text(s.to_string())
    }
    fn var(s: &str) -> Node {
        Node::Var(s.to_string())
    }

    #[test]
    fn plain_text_is_one_node() {
        assert_eq!(parse("hello").unwrap(), vec![text("hello")]);
        assert_eq!(parse("").unwrap(), Vec::<Node>::new());
    }

    #[test]
    fn dollar_names_become_vars() {
        assert_eq!(
            parse("$a b $c_d").unwrap(),
            vec![var("a"), text(" b "), var("c_d")]
        );
        assert_eq!(parse("${count}p").unwrap(), vec![var("count"), text("p")]);
    }

    #[test]
    fn escapes_produce_literals() {
        assert_eq!(parse("$$5").unwrap(), vec![text("$5")]);
        assert_eq!(parse(r"\[x\]").unwrap(), vec![text("[x]")]);
        assert_eq!(parse(r"a\$b").unwrap(), vec![text("a$b")]);
    }

    #[test]
    fn styled_run_carries_its_style_and_children() {
        let nodes = parse("[ $pr ](bg:first fg:ok)").unwrap();
        match &nodes[..] {
            [Node::Styled(children, spec)] => {
                assert_eq!(children, &vec![text(" "), var("pr"), text(" ")]);
                assert_eq!(spec, &StyleSpec::parse("bg:first fg:ok").unwrap());
            }
            other => panic!("expected one Styled node, got {other:?}"),
        }
    }

    #[test]
    fn groups_nest() {
        let nodes = parse("(a($b)c)").unwrap();
        assert_eq!(
            nodes,
            vec![Node::Group(vec![
                text("a"),
                Node::Group(vec![var("b")]),
                text("c")
            ])]
        );
    }

    #[test]
    fn styled_inside_group_inside_styled() {
        let nodes = parse("[x( [$y](bold))](fg:red)").unwrap();
        let Node::Styled(outer, _) = &nodes[0] else {
            panic!()
        };
        let Node::Group(g) = &outer[1] else { panic!() };
        assert!(matches!(&g[1], Node::Styled(inner, _) if inner == &vec![var("y")]));
    }

    #[test]
    fn errors_carry_char_offsets() {
        assert_eq!(parse("ab[cd").unwrap_err().offset, 2);
        assert_eq!(parse("a(b").unwrap_err().offset, 1);
        assert_eq!(parse("a]b").unwrap_err().offset, 1);
        assert_eq!(parse("a)b").unwrap_err().offset, 1);
        assert_eq!(parse("[x]y").unwrap_err().offset, 3);
        assert_eq!(parse("[x](bold").unwrap_err().offset, 3);
        assert_eq!(parse("a$ b").unwrap_err().offset, 1);
        assert_eq!(parse("${x").unwrap_err().offset, 0);
        assert_eq!(parse(r"ab\").unwrap_err().offset, 2);
    }

    #[test]
    fn bad_style_string_is_a_parse_error_at_the_style() {
        let err = parse("[x](fg:)").unwrap_err();
        assert_eq!(err.offset, 4);
        assert!(err.message.contains("color"), "{}", err.message);
    }

    #[test]
    fn vars_and_styles_walk_the_tree() {
        let nodes = parse("$a[$b($c)](bold)$a").unwrap();
        assert_eq!(vars(&nodes), vec!["a", "b", "c", "a"]);
        assert_eq!(styles(&nodes).len(), 1);
    }
}
```

The `bad_style_string_is_a_parse_error_at_the_style` test only passes after Task 3 replaces the stub; mark it `#[ignore = "needs style grammar (Task 3)"]` for now and remove the attribute in Task 3.

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --lib ui::bar::format`
Expected: compile errors (`parse`, `Node`, `vars`, `styles` undefined).

- [ ] **Step 4: Implement the parser**

Insert above the tests module in `src/ui/bar/format.rs`:

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    /// Literal text, rendered in the inherited style.
    Text(String),
    /// `$name` — insert the segment (or segment-local variable) `name`.
    Var(String),
    /// `[children](style)` — children rendered with `style` patched over
    /// the inherited style.
    Styled(Vec<Node>, StyleSpec),
    /// `(children)` — rendered only if a `Var` inside produced output.
    Group(Vec<Node>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// 0-based char index of the offending token.
    pub offset: usize,
    pub message: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "col {}: {}", self.offset, self.message)
    }
}

fn err(offset: usize, message: impl Into<String>) -> ParseError {
    ParseError {
        offset,
        message: message.into(),
    }
}

pub fn parse(src: &str) -> Result<Vec<Node>, ParseError> {
    let chars: Vec<char> = src.chars().collect();
    let mut pos = 0;
    let nodes = parse_seq(&chars, &mut pos, None)?;
    debug_assert_eq!(pos, chars.len());
    Ok(nodes)
}

fn flush(text: &mut String, nodes: &mut Vec<Node>) {
    if !text.is_empty() {
        nodes.push(Node::Text(std::mem::take(text)));
    }
}

/// Parse nodes until `until` (or end of input when `None`). On return
/// `pos` sits on the closer (not consumed) or at the end.
fn parse_seq(chars: &[char], pos: &mut usize, until: Option<char>) -> Result<Vec<Node>, ParseError> {
    let mut nodes = Vec::new();
    let mut text = String::new();
    while *pos < chars.len() {
        let c = chars[*pos];
        match c {
            '\\' => {
                let Some(&escaped) = chars.get(*pos + 1) else {
                    return Err(err(*pos, "dangling backslash"));
                };
                text.push(escaped);
                *pos += 2;
            }
            '$' => {
                let dollar = *pos;
                *pos += 1;
                if chars.get(*pos) == Some(&'$') {
                    text.push('$');
                    *pos += 1;
                    continue;
                }
                let name: String = if chars.get(*pos) == Some(&'{') {
                    *pos += 1;
                    let start = *pos;
                    while *pos < chars.len() && chars[*pos] != '}' {
                        *pos += 1;
                    }
                    if *pos >= chars.len() {
                        return Err(err(dollar, "unterminated `${`"));
                    }
                    let n = chars[start..*pos].iter().collect();
                    *pos += 1;
                    n
                } else {
                    let start = *pos;
                    while *pos < chars.len() && (chars[*pos].is_ascii_alphanumeric() || chars[*pos] == '_') {
                        *pos += 1;
                    }
                    chars[start..*pos].iter().collect()
                };
                if name.is_empty() {
                    return Err(err(dollar, "expected a name after `$` (write `$$` for a literal dollar)"));
                }
                flush(&mut text, &mut nodes);
                nodes.push(Node::Var(name));
            }
            '[' => {
                flush(&mut text, &mut nodes);
                let open = *pos;
                *pos += 1;
                let inner = parse_seq(chars, pos, Some(']'))?;
                if chars.get(*pos) != Some(&']') {
                    return Err(err(open, "unclosed `[`"));
                }
                *pos += 1;
                if chars.get(*pos) != Some(&'(') {
                    return Err(err(*pos, "expected `(style)` after `]`"));
                }
                let paren = *pos;
                *pos += 1;
                let start = *pos;
                while *pos < chars.len() && chars[*pos] != ')' {
                    *pos += 1;
                }
                if *pos >= chars.len() {
                    return Err(err(paren, "unclosed `(style)`"));
                }
                let style_src: String = chars[start..*pos].iter().collect();
                *pos += 1;
                let spec = StyleSpec::parse(&style_src).map_err(|e| err(start, e.to_string()))?;
                nodes.push(Node::Styled(inner, spec));
            }
            '(' => {
                flush(&mut text, &mut nodes);
                let open = *pos;
                *pos += 1;
                let inner = parse_seq(chars, pos, Some(')'))?;
                if chars.get(*pos) != Some(&')') {
                    return Err(err(open, "unclosed `(`"));
                }
                *pos += 1;
                nodes.push(Node::Group(inner));
            }
            ']' | ')' => {
                if Some(c) == until {
                    flush(&mut text, &mut nodes);
                    return Ok(nodes);
                }
                return Err(err(*pos, format!("unexpected `{c}`")));
            }
            _ => {
                text.push(c);
                *pos += 1;
            }
        }
    }
    flush(&mut text, &mut nodes);
    Ok(nodes)
}

/// Every `$name` in `nodes`, depth-first, duplicates kept.
pub fn vars(nodes: &[Node]) -> Vec<&str> {
    let mut out = Vec::new();
    for n in nodes {
        match n {
            Node::Text(_) => {}
            Node::Var(v) => out.push(v.as_str()),
            Node::Styled(children, _) | Node::Group(children) => out.extend(vars(children)),
        }
    }
    out
}

/// Every style spec in `nodes`, depth-first.
pub fn styles(nodes: &[Node]) -> Vec<&StyleSpec> {
    let mut out = Vec::new();
    for n in nodes {
        match n {
            Node::Text(_) | Node::Var(_) => {}
            Node::Styled(children, spec) => {
                out.push(spec);
                out.extend(styles(children));
            }
            Node::Group(children) => out.extend(styles(children)),
        }
    }
    out
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test --lib ui::bar::format`
Expected: all PASS except the ignored one.

- [ ] **Step 6: Commit**

```bash
git add src/ui/mod.rs src/ui/bar
git commit -m "Bar engine: format-string parser for the starship subset"
```

---

### Task 3: Style grammar (`src/ui/bar/style.rs`)

**Files:**
- Replace: `src/ui/bar/style.rs`
- Modify: `src/ui/theme.rs` (add `Theme::token`)
- Modify: `src/ui/bar/format.rs` (remove the `#[ignore]` from Task 2)

**Interfaces:**
- Produces:
  - `pub enum ColorRef { Literal(Color), Named(String) }`
  - `pub struct StyleSpec { pub fg: Option<ColorRef>, pub bg: Option<ColorRef>, pub modifiers: Modifier, pub vars: Vec<String> }` with `StyleSpec::parse(&str)`.
  - `pub fn color_ref(token: &str) -> Result<ColorRef, StyleError>` (hex `#rrggbb` → Literal Rgb, `0..=255` → Literal Indexed, identifier → Named).
  - `pub fn ansi(name: &str) -> Option<Color>`.
  - `pub struct Resolver<'a> { pub palette: &'a HashMap<String, Color>, pub theme: &'a Theme, pub styles: HashMap<String, Style> }` with `Resolver::new(palette, theme)`, `Resolver::with_styles(&self, styles) -> Resolver<'a>`, `Resolver::color(&self, name) -> Option<Color>` (palette → theme token → ansi), `Resolver::resolve(&self, &StyleSpec) -> Result<Style, StyleError>`.
  - `Theme::token(&self, name: &str) -> Option<Color>`.

- [ ] **Step 1: Write the failing tests**

Replace `src/ui/bar/style.rs` with the header plus tests (implementation added in Step 3):

```rust
//! Style-string grammar for `[text](style)` and segment `style` fields:
//! space-separated tokens `fg:<c>`, `bg:<c>`, a bare `<c>` (foreground),
//! the modifiers `bold`, `dimmed`, `italic`, `underline`, `none`, and
//! `$name` to patch in a named style (`$style`, `$mark_style`). A color is
//! `#rrggbb`, a 0–255 index, or a name resolved at load time against the
//! palette, then the theme tokens, then the ANSI names.

use crate::ui::theme::Theme;
use ratatui::style::{Color, Modifier, Style};
use std::collections::HashMap;

#[cfg(test)]
mod tests {
    use super::*;

    fn resolver<'a>(palette: &'a HashMap<String, Color>, theme: &'a Theme) -> Resolver<'a> {
        Resolver::new(palette, theme)
    }

    #[test]
    fn parses_every_token_kind() {
        let s = StyleSpec::parse("fg:#ff0000 bg:12 bold dimmed italic underline $style").unwrap();
        assert_eq!(s.fg, Some(ColorRef::Literal(Color::Rgb(0xff, 0, 0))));
        assert_eq!(s.bg, Some(ColorRef::Literal(Color::Indexed(12))));
        assert!(s.modifiers.contains(Modifier::BOLD | Modifier::DIM | Modifier::ITALIC | Modifier::UNDERLINED));
        assert_eq!(s.vars, vec!["style".to_string()]);
    }

    #[test]
    fn bare_color_is_foreground_and_none_is_empty() {
        let s = StyleSpec::parse("first").unwrap();
        assert_eq!(s.fg, Some(ColorRef::Named("first".into())));
        assert_eq!(StyleSpec::parse("none").unwrap(), StyleSpec::default());
        assert_eq!(StyleSpec::parse("").unwrap(), StyleSpec::default());
    }

    #[test]
    fn rejects_malformed_tokens() {
        assert!(StyleSpec::parse("fg:").unwrap_err().0.contains("color"));
        assert!(StyleSpec::parse("#12345").is_err());
        assert!(StyleSpec::parse("#gggggg").is_err());
        assert!(StyleSpec::parse("256").is_err());
        assert!(StyleSpec::parse("$").is_err());
        assert!(StyleSpec::parse("fg:bg:red").is_err());
    }

    #[test]
    fn resolution_order_is_palette_then_theme_then_ansi() {
        let theme = Theme::wsx();
        let mut palette = HashMap::new();
        palette.insert("dim".to_string(), Color::Rgb(1, 2, 3));
        palette.insert("red".to_string(), Color::Rgb(4, 5, 6));
        let r = resolver(&palette, &theme);
        assert_eq!(r.color("dim"), Some(Color::Rgb(1, 2, 3)), "palette shadows theme token");
        assert_eq!(r.color("red"), Some(Color::Rgb(4, 5, 6)), "palette shadows ansi");
        assert_eq!(r.color("ok"), Some(theme.ok), "theme token");
        assert_eq!(r.color("bright-blue"), Some(Color::LightBlue), "ansi");
        assert_eq!(r.color("white"), Some(Color::Gray), "ansi 7 is ratatui Gray");
        assert_eq!(r.color("nope"), None);
    }

    #[test]
    fn resolve_builds_a_ratatui_style() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let r = resolver(&palette, &theme);
        let st = r.resolve(&StyleSpec::parse("fg:ok bg:#101010 bold").unwrap()).unwrap();
        assert_eq!(st, Style::default().fg(theme.ok).bg(Color::Rgb(0x10, 0x10, 0x10)).add_modifier(Modifier::BOLD));
    }

    #[test]
    fn unknown_name_is_an_error_naming_it() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let r = resolver(&palette, &theme);
        let e = r.resolve(&StyleSpec::parse("fg:rusty").unwrap()).unwrap_err();
        assert!(e.0.contains("rusty"), "{}", e.0);
        let e = r.resolve(&StyleSpec::parse("$mark_style").unwrap()).unwrap_err();
        assert!(e.0.contains("mark_style"), "{}", e.0);
    }

    #[test]
    fn style_vars_patch_in_then_explicit_tokens_win() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let mut styles = HashMap::new();
        styles.insert("style".to_string(), Style::default().fg(Color::Red).bg(Color::Blue));
        let r = resolver(&palette, &theme).with_styles(styles);
        let st = r.resolve(&StyleSpec::parse("$style fg:ok").unwrap()).unwrap();
        assert_eq!(st.fg, Some(theme.ok));
        assert_eq!(st.bg, Some(Color::Blue));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib ui::bar::style`
Expected: compile errors.

- [ ] **Step 3: Implement**

Insert between the `use` lines and the tests module:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColorRef {
    /// `#rrggbb` or a 0–255 index: known at parse time.
    Literal(Color),
    /// A palette name, theme token, or ANSI name: resolved at load time.
    Named(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StyleSpec {
    pub fg: Option<ColorRef>,
    pub bg: Option<ColorRef>,
    pub modifiers: Modifier,
    /// `$name` tokens: named styles patched in (in order) before `fg`/`bg`/
    /// `modifiers` are applied on top.
    pub vars: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleError(pub String);

impl std::fmt::Display for StyleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Parse one color token. Names are kept unresolved.
pub fn color_ref(token: &str) -> Result<ColorRef, StyleError> {
    if let Some(hex) = token.strip_prefix('#') {
        if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(StyleError(format!("bad hex color `{token}` (want #rrggbb)")));
        }
        let v = u32::from_str_radix(hex, 16).expect("validated hex");
        return Ok(ColorRef::Literal(Color::Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)));
    }
    if !token.is_empty() && token.chars().all(|c| c.is_ascii_digit()) {
        return token
            .parse::<u8>()
            .map(|i| ColorRef::Literal(Color::Indexed(i)))
            .map_err(|_| StyleError(format!("color index `{token}` out of range (0-255)")));
    }
    if is_ident(token) {
        return Ok(ColorRef::Named(token.to_string()));
    }
    Err(StyleError(format!("bad color `{token}`")))
}

/// The 16 ANSI names, starship spelling: `red`, `bright-red`, `white` (ANSI 7,
/// ratatui `Gray`), `bright-white`, plus `gray`/`grey` and `dark-gray`.
pub fn ansi(name: &str) -> Option<Color> {
    let (bright, base) = match name.strip_prefix("bright-") {
        Some(b) => (true, b),
        None => (false, name),
    };
    Some(match (base, bright) {
        ("black", false) => Color::Black,
        ("black", true) => Color::DarkGray,
        ("red", false) => Color::Red,
        ("red", true) => Color::LightRed,
        ("green", false) => Color::Green,
        ("green", true) => Color::LightGreen,
        ("yellow", false) => Color::Yellow,
        ("yellow", true) => Color::LightYellow,
        ("blue", false) => Color::Blue,
        ("blue", true) => Color::LightBlue,
        ("magenta", false) => Color::Magenta,
        ("magenta", true) => Color::LightMagenta,
        ("cyan", false) => Color::Cyan,
        ("cyan", true) => Color::LightCyan,
        ("white", false) => Color::Gray,
        ("white", true) => Color::White,
        ("gray" | "grey", false) => Color::Gray,
        ("dark-gray" | "dark-grey", false) => Color::DarkGray,
        _ => return None,
    })
}

impl StyleSpec {
    pub fn parse(src: &str) -> Result<Self, StyleError> {
        let mut spec = Self::default();
        for tok in src.split_whitespace() {
            match tok {
                "bold" => spec.modifiers |= Modifier::BOLD,
                "dimmed" => spec.modifiers |= Modifier::DIM,
                "italic" => spec.modifiers |= Modifier::ITALIC,
                "underline" => spec.modifiers |= Modifier::UNDERLINED,
                "none" => {}
                _ => {
                    if let Some(name) = tok.strip_prefix('$') {
                        if !is_ident(name) {
                            return Err(StyleError(format!("bad style variable `{tok}`")));
                        }
                        spec.vars.push(name.to_string());
                    } else if let Some(c) = tok.strip_prefix("fg:") {
                        spec.fg = Some(color_ref(c).map_err(|e| StyleError(format!("fg: {}", e.0)))?);
                    } else if let Some(c) = tok.strip_prefix("bg:") {
                        spec.bg = Some(color_ref(c).map_err(|e| StyleError(format!("bg: {}", e.0)))?);
                    } else {
                        spec.fg = Some(color_ref(tok)?);
                    }
                }
            }
        }
        Ok(spec)
    }
}

/// Resolves color names and `$style` variables. Cheap to build per frame:
/// two borrows plus a small owned map of named styles.
#[derive(Debug, Clone)]
pub struct Resolver<'a> {
    pub palette: &'a HashMap<String, Color>,
    pub theme: &'a Theme,
    pub styles: HashMap<String, Style>,
}

impl<'a> Resolver<'a> {
    pub fn new(palette: &'a HashMap<String, Color>, theme: &'a Theme) -> Self {
        Self {
            palette,
            theme,
            styles: HashMap::new(),
        }
    }

    /// Same palette and theme, different `$name` styles.
    pub fn with_styles(&self, styles: HashMap<String, Style>) -> Resolver<'a> {
        Resolver {
            palette: self.palette,
            theme: self.theme,
            styles,
        }
    }

    /// Palette name, then theme token, then ANSI name.
    pub fn color(&self, name: &str) -> Option<Color> {
        self.palette
            .get(name)
            .copied()
            .or_else(|| self.theme.token(name))
            .or_else(|| ansi(name))
    }

    fn color_of(&self, r: &ColorRef) -> Result<Color, StyleError> {
        match r {
            ColorRef::Literal(c) => Ok(*c),
            ColorRef::Named(n) => self
                .color(n)
                .ok_or_else(|| StyleError(format!("unknown color `{n}`"))),
        }
    }

    pub fn resolve(&self, spec: &StyleSpec) -> Result<Style, StyleError> {
        let mut style = Style::default();
        for v in &spec.vars {
            let named = self
                .styles
                .get(v)
                .ok_or_else(|| StyleError(format!("unknown style variable `${v}`")))?;
            style = style.patch(*named);
        }
        if let Some(fg) = &spec.fg {
            style = style.fg(self.color_of(fg)?);
        }
        if let Some(bg) = &spec.bg {
            style = style.bg(self.color_of(bg)?);
        }
        Ok(style.add_modifier(spec.modifiers))
    }
}
```

Add to `impl Theme` in `src/ui/theme.rs`, right after `by_name`:

```rust
    /// Look up a theme color by its token name, for `theme.toml` style
    /// strings (`fg:dim`, `bg:bg_soft`). `None` for an unknown name.
    pub fn token(&self, name: &str) -> Option<Color> {
        Some(match name {
            "header_fg" => self.header_fg,
            "selected_fg" => self.selected_fg,
            "selected_bg" => self.selected_bg,
            "dim" => self.dim,
            "path" => self.path,
            "code" => self.code,
            "bg_alt" => self.bg_alt,
            "bg_soft" => self.bg_soft,
            "ok" => self.ok,
            "warn" => self.warn,
            "err" => self.err,
            "attention" => self.attention,
            "merged" => self.merged,
            "question" => self.question,
            "stalled" => self.stalled,
            "waiting" => self.waiting,
            "thinking" => self.thinking,
            "complete" => self.complete,
            "idle" => self.idle,
            "brand" => BRAND_ACCENT,
            _ => return None,
        })
    }
```

Remove the `#[ignore …]` attribute from `bad_style_string_is_a_parse_error_at_the_style` in `format.rs`.

- [ ] **Step 4: Run tests**

Run: `cargo test --lib ui::bar`
Expected: all format and style tests PASS (including the previously ignored one).

- [ ] **Step 5: Commit**

```bash
git add src/ui/bar src/ui/theme.rs
git commit -m "Bar engine: style-string grammar and name resolver"
```

---

### Task 4: Segments and the evaluator (`segment.rs`, `render.rs`)

**Files:**
- Create: `src/ui/bar/segment.rs`
- Create: `src/ui/bar/render.rs`
- Modify: `src/ui/bar/mod.rs` (declare both)

**Interfaces:**
- Produces (`segment.rs`):
  - `pub enum Hit { Key(KeyEvent), ArmLeader, PinnedChip(usize), Pr, Procs, Agent(AgentInstanceId), UsageGraph, Attention(WorkspaceId), AttentionMore }` (`Copy, Eq`)
  - `pub struct HitSpan { pub start_col: u16, pub width: u16, pub hit: Hit }`
  - `pub struct Segment { pub spans: Vec<Span<'static>>, pub width: u16, pub hits: Vec<HitSpan> }` with `Segment::text(s, style)`, `push(span)`, `append(other)`, `hit_from(start_col, hit)`, `is_empty()`, `plain_text()`.
  - `pub struct SegmentConfig { pub style: StyleSpec, pub symbol: Option<String>, pub format: Vec<Node>, pub disabled: bool, pub priority: u32, pub separator: String }`
  - `pub type SegmentMap = HashMap<String, Segment>`
- Produces (`render.rs`):
  - `pub struct BarSpec { pub format: Vec<Node>, pub right_format: Vec<Node>, pub style: StyleSpec, pub fill: String, pub fill_style: StyleSpec }`
  - `pub struct Rendered { pub line: Line<'static>, pub hits: Vec<HitSpan> }`
  - `pub fn eval(nodes: &[Node], vars: &SegmentMap, resolver: &Resolver, base: Style) -> (Segment, bool)`
  - `pub fn render_bar(spec: &BarSpec, segments: &SegmentMap, configs: &HashMap<String, SegmentConfig>, width: u16, resolver: &Resolver) -> Rendered`
  - `pub fn hit_rects(area: Rect, hits: &[HitSpan]) -> Vec<(Rect, Hit)>`
- Consumes: `Node`, `vars()` (Task 2); `StyleSpec`, `Resolver` (Task 3).

- [ ] **Step 1: Write `segment.rs`**

```rust
//! The provider contract: a rendered [`Segment`] is spans plus click hits
//! whose columns are relative to the segment's own start, so a segment can
//! be placed anywhere in a bar and its hits travel with it.

use super::format::Node;
use super::style::StyleSpec;
use crate::data::store::{AgentInstanceId, WorkspaceId};
use crossterm::event::KeyEvent;
use ratatui::style::Style;
use ratatui::text::Span;
use std::collections::HashMap;

/// Every click action a bar can carry. The input handlers keep their
/// existing per-bar rect lists; the bar composers translate hits into them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    /// Synthesize this key press (footer key hints).
    Key(KeyEvent),
    /// Arm the attached-view `^x` leader.
    ArmLeader,
    /// Fire pinned command `i` (0-based).
    PinnedChip(usize),
    /// Open the focused workspace's PR.
    Pr,
    /// Open the process-list modal.
    Procs,
    /// Focus this agent instance.
    Agent(AgentInstanceId),
    /// Open the usage-window picker.
    UsageGraph,
    /// Attach to this workspace (attention item).
    Attention(WorkspaceId),
    /// Open the updates panel (`… +N more`).
    AttentionMore,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HitSpan {
    /// Column offset from the start of the owning segment or line.
    pub start_col: u16,
    pub width: u16,
    pub hit: Hit,
}

#[derive(Debug, Clone, Default)]
pub struct Segment {
    pub spans: Vec<Span<'static>>,
    /// Display cells (not chars) — a CJK workspace name is two per char.
    pub width: u16,
    pub hits: Vec<HitSpan>,
}

impl Segment {
    pub fn text(s: impl Into<String>, style: Style) -> Self {
        let mut seg = Self::default();
        seg.push(Span::styled(s.into(), style));
        seg
    }

    pub fn push(&mut self, span: Span<'static>) {
        self.width = self.width.saturating_add(span.width() as u16);
        self.spans.push(span);
    }

    /// Append `other`, shifting its hits past this segment's current width.
    pub fn append(&mut self, other: Segment) {
        let offset = self.width;
        for h in other.hits {
            self.hits.push(HitSpan {
                start_col: h.start_col.saturating_add(offset),
                width: h.width,
                hit: h.hit,
            });
        }
        for s in other.spans {
            self.push(s);
        }
    }

    /// Record a hit covering everything pushed since column `start_col`.
    pub fn hit_from(&mut self, start_col: u16, hit: Hit) {
        self.hits.push(HitSpan {
            start_col,
            width: self.width.saturating_sub(start_col),
            hit,
        });
    }

    pub fn is_empty(&self) -> bool {
        self.width == 0
    }

    /// The spans' text concatenated (tests and error messages).
    pub fn plain_text(&self) -> String {
        self.spans.iter().map(|s| s.content.as_ref()).collect()
    }
}

/// A `[segment]` table from `theme.toml`, parsed and merged over the
/// bundled default.
#[derive(Debug, Clone, PartialEq)]
pub struct SegmentConfig {
    /// User style, merged over the provider's state-derived default to
    /// become `$style`.
    pub style: StyleSpec,
    pub symbol: Option<String>,
    pub format: Vec<Node>,
    pub disabled: bool,
    /// Overflow survival: lower is dropped first from `right_format`.
    pub priority: u32,
    /// Between items of a multi-item segment (`keys`, `pins`, `agents`).
    pub separator: String,
}

pub type SegmentMap = HashMap<String, Segment>;
```

- [ ] **Step 2: Write the failing evaluator tests**

`src/ui/bar/render.rs` header and tests (implementation in Step 4):

```rust
//! Evaluate a parsed format against a map of segments: style inheritance
//! through nested `[ ](…)`, `( )` collapse, `right_format` alignment with
//! priority-based overflow, and hit-column tracking.

use super::format::{self, Node};
use super::segment::{Hit, HitSpan, Segment, SegmentConfig, SegmentMap};
use super::style::{Resolver, StyleSpec};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use std::collections::HashMap;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::Theme;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::style::{Color, Modifier};

    fn seg(s: &str) -> Segment {
        Segment::text(s, Style::default())
    }
    fn hit_seg(s: &str, hit: Hit) -> Segment {
        let mut seg = seg(s);
        seg.hit_from(0, hit);
        seg
    }
    fn map(entries: &[(&str, Segment)]) -> SegmentMap {
        entries.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
    }
    fn spec(format: &str, right: &str, fill: &str) -> BarSpec {
        BarSpec {
            format: format::parse(format).unwrap(),
            right_format: format::parse(right).unwrap(),
            style: StyleSpec::default(),
            fill: fill.to_string(),
            fill_style: StyleSpec::default(),
        }
    }
    fn cfg(priority: u32) -> SegmentConfig {
        SegmentConfig {
            style: StyleSpec::default(),
            symbol: None,
            format: vec![],
            disabled: false,
            priority,
            separator: String::new(),
        }
    }
    fn text(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }
    const KEY: Hit = Hit::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));

    #[test]
    fn group_collapses_when_no_var_inside_produced_output() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let r = Resolver::new(&palette, &theme);
        let nodes = format::parse("a( $b)c").unwrap();
        let (out, _) = eval(&nodes, &map(&[]), &r, Style::default());
        assert_eq!(out.plain_text(), "ac");
        let (out, _) = eval(&nodes, &map(&[("b", seg("B"))]), &r, Style::default());
        assert_eq!(out.plain_text(), "a Bc");
        // An empty segment counts as absent.
        let (out, _) = eval(&nodes, &map(&[("b", seg(""))]), &r, Style::default());
        assert_eq!(out.plain_text(), "ac");
    }

    #[test]
    fn nested_group_propagates_output_upward() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let r = Resolver::new(&palette, &theme);
        let nodes = format::parse("(x($b))").unwrap();
        assert_eq!(eval(&nodes, &map(&[]), &r, Style::default()).0.plain_text(), "");
        assert_eq!(
            eval(&nodes, &map(&[("b", seg("B"))]), &r, Style::default()).0.plain_text(),
            "xB"
        );
    }

    #[test]
    fn styles_inherit_inward_and_var_spans_keep_their_fg() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let r = Resolver::new(&palette, &theme);
        let nodes = format::parse("[a[b](fg:red)$v](bg:blue)").unwrap();
        let v = Segment::text("V", Style::default().fg(Color::Green));
        let (out, _) = eval(&nodes, &map(&[("v", v)]), &r, Style::default());
        assert_eq!(out.spans[0].style, Style::default().bg(Color::Blue));
        assert_eq!(out.spans[1].style, Style::default().bg(Color::Blue).fg(Color::Red));
        assert_eq!(out.spans[2].style, Style::default().bg(Color::Blue).fg(Color::Green));
    }

    #[test]
    fn hit_columns_count_cells_not_chars() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let r = Resolver::new(&palette, &theme);
        let nodes = format::parse("$w $k").unwrap();
        let (out, _) = eval(
            &nodes,
            &map(&[("w", seg("日本")), ("k", hit_seg("go", KEY))]),
            &r,
            Style::default(),
        );
        assert_eq!(out.width, 7);
        assert_eq!(out.hits, vec![HitSpan { start_col: 5, width: 2, hit: KEY }]);
    }

    #[test]
    fn right_format_is_flush_right_with_fill_between() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let r = Resolver::new(&palette, &theme);
        let out = render_bar(
            &spec("$a", "$b", "─"),
            &map(&[("a", seg("left")), ("b", hit_seg("right", Hit::Pr))]),
            &HashMap::new(),
            20,
            &r,
        );
        assert_eq!(text(&out.line), "left───────────right");
        assert_eq!(out.hits, vec![HitSpan { start_col: 15, width: 5, hit: Hit::Pr }]);
    }

    #[test]
    fn fill_extends_to_the_edge_when_right_is_empty() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let r = Resolver::new(&palette, &theme);
        let out = render_bar(&spec("$a", "$b", "─"), &map(&[("a", seg("left"))]), &HashMap::new(), 10, &r);
        assert_eq!(text(&out.line), "left──────");
    }

    #[test]
    fn overflow_drops_lowest_priority_right_segment_and_reevaluates_groups() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let r = Resolver::new(&palette, &theme);
        let mut configs = HashMap::new();
        configs.insert("b".to_string(), cfg(10));
        configs.insert("c".to_string(), cfg(50));
        let segs = map(&[("a", seg("left")), ("b", hit_seg("bbbb", Hit::Procs)), ("c", seg("cc"))]);
        // 4 + 7 = 11 > 10: drop `b` (priority 10), and its group's space goes with it.
        let out = render_bar(&spec("$a", "($b )$c", " "), &segs, &configs, 10, &r);
        assert_eq!(text(&out.line), "left    cc");
        assert!(out.hits.is_empty(), "the dropped segment's hit is gone");
        // Wide enough: nothing dropped.
        let out = render_bar(&spec("$a", "($b )$c", " "), &segs, &configs, 12, &r);
        assert_eq!(text(&out.line), "left bbbb cc");
    }

    #[test]
    fn left_side_is_never_dropped_only_clipped_by_the_caller() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let r = Resolver::new(&palette, &theme);
        let out = render_bar(&spec("$a", "$c", " "), &map(&[("a", seg("left")), ("c", seg("cc"))]), &HashMap::new(), 3, &r);
        assert_eq!(text(&out.line), "left");
    }

    #[test]
    fn bar_style_is_the_base_for_text_and_fill() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let r = Resolver::new(&palette, &theme);
        let mut s = spec("x", "y", "-");
        s.style = StyleSpec::parse("bg:blue").unwrap();
        s.fill_style = StyleSpec::parse("fg:red").unwrap();
        let out = render_bar(&s, &map(&[]), &HashMap::new(), 4, &r);
        assert_eq!(text(&out.line), "x--y");
        assert_eq!(out.line.spans[0].style, Style::default().bg(Color::Blue));
        assert_eq!(out.line.spans[1].style, Style::default().bg(Color::Blue).fg(Color::Red));
        assert!(!out.line.spans[0].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn hit_rects_are_absolute_and_clipped() {
        let area = Rect::new(10, 5, 8, 1);
        let hits = vec![
            HitSpan { start_col: 2, width: 3, hit: Hit::Pr },
            HitSpan { start_col: 6, width: 5, hit: Hit::Procs },
            HitSpan { start_col: 9, width: 1, hit: Hit::ArmLeader },
        ];
        let rects = hit_rects(area, &hits);
        assert_eq!(rects[0], (Rect::new(12, 5, 3, 1), Hit::Pr));
        assert_eq!(rects[1], (Rect::new(16, 5, 2, 1), Hit::Procs), "clipped at the right edge");
        assert_eq!(rects.len(), 2, "a hit past the edge is dropped");
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --lib ui::bar::render`
Expected: compile errors.

- [ ] **Step 4: Implement the evaluator**

Insert between the `use` lines and the tests module of `render.rs`:

```rust
/// One bar's parsed `[bar]` table.
#[derive(Debug, Clone, PartialEq)]
pub struct BarSpec {
    pub format: Vec<Node>,
    pub right_format: Vec<Node>,
    /// Base style for literal text and the fill; inherited by everything.
    pub style: StyleSpec,
    /// Repeated (first char) across the gap between `format` and
    /// `right_format`. Empty means spaces.
    pub fill: String,
    pub fill_style: StyleSpec,
}

#[derive(Debug, Clone)]
pub struct Rendered {
    pub line: Line<'static>,
    /// Columns relative to the line's first cell.
    pub hits: Vec<HitSpan>,
}

/// Restyle a provider segment for insertion: the inherited `base` under
/// each span's own style, so `bg` flows in while a state-derived `fg`
/// stays put.
fn inherit(seg: &Segment, base: Style) -> Segment {
    Segment {
        spans: seg
            .spans
            .iter()
            .map(|s| Span::styled(s.content.clone(), base.patch(s.style)))
            .collect(),
        width: seg.width,
        hits: seg.hits.clone(),
    }
}

/// Evaluate `nodes`. The bool is "some `$var` produced output", which is
/// what a `( )` group keys on. Names are validated at load time, so a
/// style that fails to resolve here silently falls back to `base`.
pub fn eval(nodes: &[Node], vars: &SegmentMap, resolver: &Resolver, base: Style) -> (Segment, bool) {
    let mut out = Segment::default();
    let mut produced = false;
    for node in nodes {
        match node {
            Node::Text(t) => out.push(Span::styled(t.clone(), base)),
            Node::Var(name) => {
                if let Some(seg) = vars.get(name).filter(|s| !s.is_empty()) {
                    produced = true;
                    out.append(inherit(seg, base));
                }
            }
            Node::Styled(children, spec) => {
                let style = base.patch(resolver.resolve(spec).unwrap_or_default());
                let (inner, p) = eval(children, vars, resolver, style);
                produced |= p;
                out.append(inner);
            }
            Node::Group(children) => {
                let (inner, p) = eval(children, vars, resolver, base);
                if p {
                    produced = true;
                    out.append(inner);
                }
            }
        }
    }
    (out, produced)
}

fn fill_run(fill: &str, cells: u16) -> String {
    let ch = fill.chars().next().unwrap_or(' ');
    std::iter::repeat_n(ch, cells as usize).collect()
}

/// Lay out one bar: `format` on the left (never dropped), `right_format`
/// flush right, the gap painted with `fill`. When both don't fit, the
/// lowest-priority segment present in `right_format` is removed and the
/// right side re-evaluated (so groups shed their separators) until it fits
/// or is empty.
pub fn render_bar(
    spec: &BarSpec,
    segments: &SegmentMap,
    configs: &HashMap<String, SegmentConfig>,
    width: u16,
    resolver: &Resolver,
) -> Rendered {
    let base = resolver.resolve(&spec.style).unwrap_or_default();
    let (left, _) = eval(&spec.format, segments, resolver, base);

    let mut vars = segments.clone();
    let (mut right, _) = eval(&spec.right_format, &vars, resolver, base);
    while !right.is_empty() && left.width.saturating_add(right.width) > width {
        let victim = format::vars(&spec.right_format)
            .into_iter()
            .filter(|n| vars.get(*n).is_some_and(|s| !s.is_empty()))
            .min_by_key(|n| configs.get(*n).map(|c| c.priority).unwrap_or(100))
            .map(str::to_string);
        let Some(victim) = victim else { break };
        vars.remove(&victim);
        right = eval(&spec.right_format, &vars, resolver, base).0;
    }

    let gap = width.saturating_sub(left.width.saturating_add(right.width));
    let mut out = left;
    if gap > 0 {
        let fill_style = base.patch(resolver.resolve(&spec.fill_style).unwrap_or_default());
        out.push(Span::styled(fill_run(&spec.fill, gap), fill_style));
    }
    out.append(right);
    Rendered {
        line: Line::from(out.spans),
        hits: out.hits,
    }
}

/// Convert line-relative hits into absolute screen rects, clipped to
/// `area`; hits entirely past the right edge are dropped.
pub fn hit_rects(area: Rect, hits: &[HitSpan]) -> Vec<(Rect, Hit)> {
    let max_x = area.x.saturating_add(area.width);
    hits.iter()
        .filter_map(|h| {
            let x = area.x.saturating_add(h.start_col);
            if x >= max_x {
                return None;
            }
            Some((
                Rect {
                    x,
                    y: area.y,
                    width: h.width.min(max_x - x),
                    height: 1,
                },
                h.hit,
            ))
        })
        .collect()
}
```

Add `pub mod render;` and `pub mod segment;` to `src/ui/bar/mod.rs`.

- [ ] **Step 5: Run tests**

Run: `cargo test --lib ui::bar`
Expected: all PASS. If `std::iter::repeat_n` is unavailable on the toolchain, use `ch.to_string().repeat(cells as usize)`.

- [ ] **Step 6: Commit**

```bash
git add src/ui/bar
git commit -m "Bar engine: segments with click hits, and the format evaluator"
```

---

### Task 5: The bundled default and the file loader (`theme_file.rs`)

**Files:**
- Create: `src/ui/bar/default_theme.toml`
- Create: `src/config/theme_file.rs`
- Modify: `src/config/mod.rs` (add `pub mod theme_file;`)

**Interfaces:**
- Produces:
  - `pub const DEFAULT_TOML: &str`
  - `pub struct ThemeFile { pub palette: BTreeMap<String,String>, pub dashboard_footer: BarTable, pub attached_top: BarTable, pub attached_bottom: BarTable, pub segments: BTreeMap<String, SegmentTable> }` (serde), `ThemeFile::parse(&str) -> Result<ThemeFile, ThemeError>`, `ThemeFile::merge_over(self, base) -> ThemeFile`
  - `pub struct BarTable { pub format, right_format, style, fill, fill_style: Option<String> }`
  - `pub struct SegmentTable { pub format, style, symbol, separator: Option<String>, pub disabled: Option<bool>, pub priority: Option<u32> }`
  - `pub struct ThemeError { pub location: String, pub message: String }` (Display `{location}: {message}`)
  - `pub struct BarSpecs { pub palette: HashMap<String, Color>, pub dashboard_footer: BarSpec, pub attached_top: BarSpec, pub attached_bottom: BarSpec, pub segments: HashMap<String, SegmentConfig> }` with `BarSpecs::resolver<'a>(&'a self, theme: &'a Theme) -> Resolver<'a>`
  - `pub fn resolve(file: ThemeFile, theme: &Theme) -> Result<BarSpecs, Vec<ThemeError>>`
  - `pub fn load(path: &Path, theme: &Theme) -> Result<BarSpecs, Vec<ThemeError>>` (missing file → bundled default)
  - `pub fn bundled_default(theme: &Theme) -> BarSpecs`
  - `pub const SEGMENTS: &[SegmentDef]` where `pub struct SegmentDef { pub name: &'static str, pub vars: &'static [&'static str], pub style_vars: &'static [&'static str] }`

- [ ] **Step 1: Write the bundled default**

`src/ui/bar/default_theme.toml`. Every value here reproduces today's rendering; Tasks 7–9 prove it.

```toml
# wsx bar theme — the bundled default. `wsx theme init` copies this to
# ~/.config/wsx/theme.toml; edit it there and wsx reloads it while running.
# Anything you leave out keeps the value below.
#
# Grammar (a subset of starship's):
#   $name / ${name}   insert a segment (in a [segment] table: a variable)
#   [text](style)     style a run; nested runs inherit the outer bg
#   ( ... )           render only if a $segment inside is non-empty
#   $$  \[  \(        literal characters
#   style tokens      fg:<c> bg:<c> <c> bold dimmed italic underline none $style
#   colors            #rrggbb, 0-255, ansi names (red, bright-blue, white),
#                     [palette] names, theme tokens: dim path code bg_alt bg_soft
#                     ok warn err attention merged header_fg selected_fg
#                     selected_bg question stalled waiting thinking complete
#                     idle brand
#
# Bars take `format`, `right_format` (flush right; segments are dropped by
# `priority`, lowest first, when the bar is too narrow), `style`, `fill`,
# `fill_style`. Segments take `format`, `style` (merged over the segment's
# own state color, available as $style), `symbol`, `disabled`, `priority`,
# and `separator` (between items of keys/pins/agents).

[palette]

[dashboard_footer]
format       = "$keys"
right_format = "$version  $usage"
fill         = " "

[attached_top]
format = "($agent_bar )$workspace(   $attention)"

[attached_bottom]
format       = "$keys  ($pins  )"
right_format = "(  ($agents   )($model_tokens )($procs )($diff )$pr)"
fill         = "─"
fill_style   = "fg:dim"

# --- segments ---------------------------------------------------------

# Key hints: one pill per binding. Variables: $key $label
[keys]
format    = "[ $key ](bg:bg_soft fg:dim bold)[ $label](fg:path)"
separator = "  "

# Variables: $version
[version]
format = "[$version](fg:path)"

# Usage sparkline. Variables: $label $spark
[usage]
format = "[$label $spark](fg:path)"

# Agent identity bar; $style carries the agent's color. Variables: $symbol
[agent_bar]
symbol = "▎"
format = "[$symbol]($style)"

# Focused workspace. Variables: $repo $name
[workspace]
format = "[($repo/)$name]($style)"

# Cross-workspace attention items. Variables: $items
[attention]
format = "$items"

# Pinned commands: one chip per command. Variables: $index $label
[pins]
format    = "[ $index ](bg:bg_soft fg:dim bold)[ $label](fg:path)"
separator = "  "

# Agent pills (2+ agents). $style is the agent color. Variables: $symbol $label $key
[agents]
format    = "[$symbol]($style)$label ([ $key ](bg:bg_soft fg:dim bold))"
separator = "   "
priority  = 20

# Variables: $model $tokens ($style is ok, or warn near the limit)
[model_tokens]
format   = "([$model](fg:code) )[$tokens]($style)"
priority = 10

# Running processes. Variables: $symbol $count
[procs]
symbol   = "●"
format   = "[$symbol ${count}p]($style)"
priority = 30

# Variables: $added $removed
[diff]
format   = "[+$added](fg:ok) [−$removed](fg:err)"
priority = 40

# PR chip; $style is the lifecycle color, $mark_style the review verdict.
# Variables: $symbol $number $label $mark
[pr]
format   = "[$symbol #$number $label]($style)( [$mark]($mark_style))"
priority = 50
```

- [ ] **Step 2: Write the failing loader tests**

`src/config/theme_file.rs` header and tests (implementation in Step 4):

```rust
//! `~/.config/wsx/theme.toml`: serde model, merge over the bundled default,
//! validation of every format string and color name, and the resolved
//! [`BarSpecs`] the renderer draws from. Parsing happens here, once per
//! load; drawing never parses.
//!
//! See `docs/superpowers/specs/2026-09-13-bar-theming-design.md`.

use crate::ui::bar::format::{self, Node};
use crate::ui::bar::render::BarSpec;
use crate::ui::bar::segment::SegmentConfig;
use crate::ui::bar::style::{self, ColorRef, Resolver, StyleSpec};
use crate::ui::theme::Theme;
use ratatui::style::{Color, Style};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

#[cfg(test)]
mod tests {
    use super::*;

    fn errs(src: &str) -> Vec<ThemeError> {
        resolve(ThemeFile::parse(src).unwrap(), &Theme::wsx()).unwrap_err()
    }
    fn ok(src: &str) -> BarSpecs {
        resolve(ThemeFile::parse(src).unwrap(), &Theme::wsx()).unwrap()
    }

    #[test]
    fn bundled_default_parses_and_validates() {
        let specs = bundled_default(&Theme::wsx());
        assert_eq!(specs.dashboard_footer.format, format::parse("$keys").unwrap());
        assert_eq!(specs.attached_bottom.fill, "─");
        assert_eq!(specs.segments["pr"].priority, 50);
        assert_eq!(specs.segments["keys"].separator, "  ");
        assert_eq!(specs.segments["procs"].symbol.as_deref(), Some("●"));
        for def in SEGMENTS {
            assert!(specs.segments.contains_key(def.name), "default lacks [{}]", def.name);
        }
    }

    #[test]
    fn partial_file_merges_per_field_over_the_default() {
        let specs = ok("[attached_top]\nformat = \"$workspace\"\n[pr]\npriority = 7\n");
        assert_eq!(specs.attached_top.format, format::parse("$workspace").unwrap());
        assert_eq!(specs.dashboard_footer.format, format::parse("$keys").unwrap());
        assert_eq!(specs.segments["pr"].priority, 7);
        assert_eq!(
            specs.segments["pr"].format,
            format::parse("[$symbol #$number $label]($style)( [$mark]($mark_style))").unwrap(),
            "unset fields keep the default"
        );
    }

    #[test]
    fn palette_accepts_literals_theme_tokens_and_ansi() {
        let specs = ok("[palette]\nfirst = \"#123456\"\nsecond = \"dim\"\nthird = \"bright-red\"\n");
        assert_eq!(specs.palette["first"], Color::Rgb(0x12, 0x34, 0x56));
        assert_eq!(specs.palette["second"], Theme::wsx().dim);
        assert_eq!(specs.palette["third"], Color::LightRed);
    }

    #[test]
    fn palette_names_resolve_in_styles() {
        let specs = ok("[palette]\nfirst = \"#123456\"\n[attached_top]\nformat = \"[$workspace](bg:first)\"\n");
        let theme = Theme::wsx();
        let r = specs.resolver(&theme);
        let Node::Styled(_, spec) = &specs.attached_top.format[0] else { panic!() };
        assert_eq!(r.resolve(spec).unwrap().bg, Some(Color::Rgb(0x12, 0x34, 0x56)));
    }

    #[test]
    fn every_error_is_reported_with_a_location() {
        let e = errs(concat!(
            "[palette]\nbad = \"#12\"\n",
            "[bogus]\nformat = \"x\"\n",
            "[dashboard_footer]\nformat = \"$nope [x](fg:rusty)\"\n",
            "[diff]\nformat = \"$foo [x]($mark_style)\"\n",
        ));
        let locs: Vec<&str> = e.iter().map(|e| e.location.as_str()).collect();
        assert!(locs.contains(&"[palette].bad"), "{locs:?}");
        assert!(locs.contains(&"[bogus]"), "{locs:?}");
        assert!(locs.iter().filter(|l| **l == "[dashboard_footer].format").count() >= 2, "{locs:?}");
        assert!(locs.iter().filter(|l| **l == "[diff].format").count() >= 2, "{locs:?}");
        let msgs: Vec<&str> = e.iter().map(|e| e.message.as_str()).collect();
        assert!(msgs.iter().any(|m| m.contains("nope")), "{msgs:?}");
        assert!(msgs.iter().any(|m| m.contains("rusty")), "{msgs:?}");
        assert!(msgs.iter().any(|m| m.contains("foo")), "{msgs:?}");
        assert!(msgs.iter().any(|m| m.contains("mark_style")), "{msgs:?}");
    }

    #[test]
    fn segment_style_vars_are_per_segment() {
        assert!(resolve(ThemeFile::parse("[pr]\nformat = \"[$mark]($mark_style)\"\n").unwrap(), &Theme::wsx()).is_ok());
        assert!(!errs("[procs]\nformat = \"[$count]($mark_style)\"\n").is_empty());
    }

    #[test]
    fn bar_formats_may_only_reference_segments() {
        let e = errs("[attached_top]\nformat = \"$symbol\"\n");
        assert_eq!(e.len(), 1);
        assert!(e[0].message.contains("symbol"));
    }

    #[test]
    fn segment_style_and_bar_style_are_validated() {
        assert!(!errs("[pr]\nstyle = \"fg:nope\"\n").is_empty());
        assert!(!errs("[attached_top]\nstyle = \"bg:\"\n").is_empty());
        assert!(!errs("[attached_top]\nfill_style = \"fg:zzz\"\n").is_empty());
    }

    #[test]
    fn bad_toml_is_one_error() {
        let e = ThemeFile::parse("[pr\nformat = 1").unwrap_err();
        assert_eq!(e.location, "toml");
    }

    #[test]
    fn load_missing_file_is_the_bundled_default() {
        let dir = tempfile::tempdir().unwrap();
        let specs = load(&dir.path().join("theme.toml"), &Theme::wsx()).unwrap();
        assert_eq!(specs.dashboard_footer.format, format::parse("$keys").unwrap());
    }

    #[test]
    fn load_reads_and_validates_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("theme.toml");
        std::fs::write(&path, "[dashboard_footer]\nformat = \"$version\"\n").unwrap();
        let specs = load(&path, &Theme::wsx()).unwrap();
        assert_eq!(specs.dashboard_footer.format, format::parse("$version").unwrap());
        std::fs::write(&path, "[dashboard_footer]\nformat = \"$nope\"\n").unwrap();
        assert!(load(&path, &Theme::wsx()).is_err());
    }
}
```

Add `pub mod theme_file;` to `src/config/mod.rs`.

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --lib config::theme_file`
Expected: compile errors.

- [ ] **Step 4: Implement the loader**

Insert between the `use` lines and the tests module:

```rust
pub const DEFAULT_TOML: &str = include_str!("../ui/bar/default_theme.toml");

/// What a segment's own `format` may reference.
pub struct SegmentDef {
    pub name: &'static str,
    pub vars: &'static [&'static str],
    pub style_vars: &'static [&'static str],
}

const STYLE: &[&str] = &["style"];

pub const SEGMENTS: &[SegmentDef] = &[
    SegmentDef { name: "keys", vars: &["key", "label"], style_vars: STYLE },
    SegmentDef { name: "version", vars: &["version"], style_vars: STYLE },
    SegmentDef { name: "usage", vars: &["label", "spark"], style_vars: STYLE },
    SegmentDef { name: "agent_bar", vars: &["symbol"], style_vars: STYLE },
    SegmentDef { name: "workspace", vars: &["repo", "name"], style_vars: STYLE },
    SegmentDef { name: "attention", vars: &["items"], style_vars: STYLE },
    SegmentDef { name: "pins", vars: &["index", "label"], style_vars: STYLE },
    SegmentDef { name: "agents", vars: &["symbol", "label", "key"], style_vars: STYLE },
    SegmentDef { name: "model_tokens", vars: &["model", "tokens"], style_vars: STYLE },
    SegmentDef { name: "procs", vars: &["symbol", "count"], style_vars: STYLE },
    SegmentDef { name: "diff", vars: &["added", "removed"], style_vars: STYLE },
    SegmentDef { name: "pr", vars: &["symbol", "number", "label", "mark"], style_vars: &["style", "mark_style"] },
];

pub fn segment_def(name: &str) -> Option<&'static SegmentDef> {
    SEGMENTS.iter().find(|d| d.name == name)
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ThemeFile {
    #[serde(default)]
    pub palette: BTreeMap<String, String>,
    #[serde(default)]
    pub dashboard_footer: BarTable,
    #[serde(default)]
    pub attached_top: BarTable,
    #[serde(default)]
    pub attached_bottom: BarTable,
    /// Every other top-level table is a `[segment]`.
    #[serde(flatten)]
    pub segments: BTreeMap<String, SegmentTable>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BarTable {
    pub format: Option<String>,
    pub right_format: Option<String>,
    pub style: Option<String>,
    pub fill: Option<String>,
    pub fill_style: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SegmentTable {
    pub format: Option<String>,
    pub style: Option<String>,
    pub symbol: Option<String>,
    pub disabled: Option<bool>,
    pub priority: Option<u32>,
    pub separator: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeError {
    /// `[table].field`, `[table]`, `toml`, or a path.
    pub location: String,
    pub message: String,
}

impl std::fmt::Display for ThemeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.location, self.message)
    }
}

fn error(location: impl Into<String>, message: impl Into<String>) -> ThemeError {
    ThemeError {
        location: location.into(),
        message: message.into(),
    }
}

impl BarTable {
    fn merge_over(self, base: BarTable) -> BarTable {
        BarTable {
            format: self.format.or(base.format),
            right_format: self.right_format.or(base.right_format),
            style: self.style.or(base.style),
            fill: self.fill.or(base.fill),
            fill_style: self.fill_style.or(base.fill_style),
        }
    }
}

impl SegmentTable {
    fn merge_over(self, base: SegmentTable) -> SegmentTable {
        SegmentTable {
            format: self.format.or(base.format),
            style: self.style.or(base.style),
            symbol: self.symbol.or(base.symbol),
            disabled: self.disabled.or(base.disabled),
            priority: self.priority.or(base.priority),
            separator: self.separator.or(base.separator),
        }
    }
}

impl ThemeFile {
    pub fn parse(src: &str) -> Result<Self, ThemeError> {
        toml::from_str(src).map_err(|e| error("toml", e.to_string()))
    }

    /// Fill every unset field from `base`; palette entries and segment
    /// tables are unioned, with `self` winning per field.
    pub fn merge_over(mut self, base: ThemeFile) -> ThemeFile {
        for (k, v) in base.palette {
            self.palette.entry(k).or_insert(v);
        }
        self.dashboard_footer = self.dashboard_footer.merge_over(base.dashboard_footer);
        self.attached_top = self.attached_top.merge_over(base.attached_top);
        self.attached_bottom = self.attached_bottom.merge_over(base.attached_bottom);
        for (name, tbl) in base.segments {
            let mine = self.segments.remove(&name).unwrap_or_default();
            self.segments.insert(name, mine.merge_over(tbl));
        }
        self
    }
}

/// Fully parsed and validated bar theme. Owned by `App`; rebuilt on reload.
#[derive(Debug, Clone)]
pub struct BarSpecs {
    pub palette: HashMap<String, Color>,
    pub dashboard_footer: BarSpec,
    pub attached_top: BarSpec,
    pub attached_bottom: BarSpec,
    pub segments: HashMap<String, SegmentConfig>,
}

impl BarSpecs {
    pub fn resolver<'a>(&'a self, theme: &'a Theme) -> Resolver<'a> {
        Resolver::new(&self.palette, theme)
    }
}

fn parse_format(loc: &str, src: &str, errors: &mut Vec<ThemeError>) -> Vec<Node> {
    match format::parse(src) {
        Ok(nodes) => nodes,
        Err(e) => {
            errors.push(error(loc, e.to_string()));
            Vec::new()
        }
    }
}

fn parse_style(loc: &str, src: Option<&str>, errors: &mut Vec<ThemeError>) -> StyleSpec {
    match StyleSpec::parse(src.unwrap_or("")) {
        Ok(s) => s,
        Err(e) => {
            errors.push(error(loc, e.to_string()));
            StyleSpec::default()
        }
    }
}

/// Check every `$var` in `nodes` is in `allowed`, and every style resolves
/// against `resolver` (which carries the allowed `$style` names).
fn validate(loc: &str, nodes: &[Node], allowed: &[&str], resolver: &Resolver, errors: &mut Vec<ThemeError>) {
    for v in format::vars(nodes) {
        if !allowed.contains(&v) {
            errors.push(error(loc, format!("unknown `${v}` (allowed: {})", allowed.join(", "))));
        }
    }
    for spec in format::styles(nodes) {
        if let Err(e) = resolver.resolve(spec) {
            errors.push(error(loc, e.to_string()));
        }
    }
}

fn placeholder_styles(names: &[&str]) -> HashMap<String, Style> {
    names.iter().map(|n| (n.to_string(), Style::default())).collect()
}

/// Merge `file` over the bundled default and resolve it. Every problem is
/// reported, not just the first.
pub fn resolve(file: ThemeFile, theme: &Theme) -> Result<BarSpecs, Vec<ThemeError>> {
    let base = ThemeFile::parse(DEFAULT_TOML).expect("bundled default_theme.toml parses");
    let file = file.merge_over(base);
    let mut errors = Vec::new();

    let mut palette = HashMap::new();
    for (name, value) in &file.palette {
        let loc = format!("[palette].{name}");
        match style::color_ref(value) {
            Ok(ColorRef::Literal(c)) => {
                palette.insert(name.clone(), c);
            }
            Ok(ColorRef::Named(n)) => match theme.token(&n).or_else(|| style::ansi(&n)) {
                Some(c) => {
                    palette.insert(name.clone(), c);
                }
                None => errors.push(error(loc, format!("unknown color `{value}`"))),
            },
            Err(e) => errors.push(error(loc, e.to_string())),
        }
    }
    let resolver = Resolver::new(&palette, theme);

    let mut segments = HashMap::new();
    for (name, tbl) in &file.segments {
        let Some(def) = segment_def(name) else {
            errors.push(error(format!("[{name}]"), format!(
                "unknown segment (known: {})",
                SEGMENTS.iter().map(|d| d.name).collect::<Vec<_>>().join(", ")
            )));
            continue;
        };
        let loc = format!("[{name}].format");
        let nodes = parse_format(&loc, tbl.format.as_deref().unwrap_or(""), &mut errors);
        let seg_resolver = resolver.with_styles(placeholder_styles(def.style_vars));
        validate(&loc, &nodes, def.vars, &seg_resolver, &mut errors);
        let style = parse_style(&format!("[{name}].style"), tbl.style.as_deref(), &mut errors);
        if let Err(e) = resolver.resolve(&style) {
            errors.push(error(format!("[{name}].style"), e.to_string()));
        }
        segments.insert(
            name.clone(),
            SegmentConfig {
                style,
                symbol: tbl.symbol.clone(),
                format: nodes,
                disabled: tbl.disabled.unwrap_or(false),
                priority: tbl.priority.unwrap_or(100),
                separator: tbl.separator.clone().unwrap_or_else(|| "  ".to_string()),
            },
        );
    }

    let segment_names: Vec<&str> = SEGMENTS.iter().map(|d| d.name).collect();
    let mut bar = |name: &str, tbl: &BarTable| -> BarSpec {
        let f_loc = format!("[{name}].format");
        let format_nodes = parse_format(&f_loc, tbl.format.as_deref().unwrap_or(""), &mut errors);
        validate(&f_loc, &format_nodes, &segment_names, &resolver, &mut errors);
        let r_loc = format!("[{name}].right_format");
        let right_nodes = parse_format(&r_loc, tbl.right_format.as_deref().unwrap_or(""), &mut errors);
        validate(&r_loc, &right_nodes, &segment_names, &resolver, &mut errors);
        let style = parse_style(&format!("[{name}].style"), tbl.style.as_deref(), &mut errors);
        if let Err(e) = resolver.resolve(&style) {
            errors.push(error(format!("[{name}].style"), e.to_string()));
        }
        let fill_style = parse_style(&format!("[{name}].fill_style"), tbl.fill_style.as_deref(), &mut errors);
        if let Err(e) = resolver.resolve(&fill_style) {
            errors.push(error(format!("[{name}].fill_style"), e.to_string()));
        }
        BarSpec {
            format: format_nodes,
            right_format: right_nodes,
            style,
            fill: tbl.fill.clone().unwrap_or_else(|| " ".to_string()),
            fill_style,
        }
    };
    let dashboard_footer = bar("dashboard_footer", &file.dashboard_footer);
    let attached_top = bar("attached_top", &file.attached_top);
    let attached_bottom = bar("attached_bottom", &file.attached_bottom);

    if errors.is_empty() {
        Ok(BarSpecs {
            palette,
            dashboard_footer,
            attached_top,
            attached_bottom,
            segments,
        })
    } else {
        Err(errors)
    }
}

/// The bundled default as resolved specs. Panics only if the embedded
/// TOML is broken, which `bundled_default_parses_and_validates` guards.
pub fn bundled_default(theme: &Theme) -> BarSpecs {
    resolve(ThemeFile::default(), theme).expect("bundled default_theme.toml validates")
}

/// Load `path` merged over the bundled default. A missing file is the
/// bundled default; any other read error, parse error, or validation
/// error is returned.
pub fn load(path: &Path, theme: &Theme) -> Result<BarSpecs, Vec<ThemeError>> {
    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(bundled_default(theme)),
        Err(e) => return Err(vec![error(path.display().to_string(), e.to_string())]),
    };
    let file = ThemeFile::parse(&src).map_err(|e| vec![e])?;
    resolve(file, theme)
}
```

Note the closure `bar` borrows `errors` mutably; the three calls happen before `errors.is_empty()`, so the borrow ends in time. If the borrow checker objects, turn `bar` into a free function taking `&mut Vec<ThemeError>` and `&Resolver`.

- [ ] **Step 5: Run tests**

Run: `cargo test --lib config::theme_file`
Expected: all PASS. If `#[serde(flatten)]` into `BTreeMap<String, SegmentTable>` rejects the `[palette]` table (some serde versions try known fields after the flatten), move `palette` above the flatten field, which is already the case; if it still fails, replace the flatten with a manual `Deserialize` that reads a `toml::Table` and splits known keys from segment tables.

- [ ] **Step 6: Commit**

```bash
git add src/config src/ui/bar/default_theme.toml
git commit -m "Bar theme: bundled default and theme.toml loader with validation"
```

---

### Task 6: `wsx theme check | path | init`

**Files:**
- Modify: `src/cli/action.rs` (three variants)
- Modify: `src/cli/groups.rs` (a `theme` group)
- Create: `src/cli/parse/theme.rs`
- Modify: `src/cli/parse/mod.rs` (module + dispatch arm)
- Modify: `src/cli/run.rs` (three handlers)
- Modify: `src/cli/tests.rs` (parse tests; add `"theme"` to `registry_matches_dispatched_groups`)

**Interfaces:**
- Produces: `CliAction::ThemeCheck { path: Option<PathBuf> }`, `CliAction::ThemePath`, `CliAction::ThemeInit`.
- Consumes: `Dirs::theme_path()` (Task 1); `theme_file::{load, DEFAULT_TOML}` (Task 5). `run_cli` already receives `&Dirs`.

- [ ] **Step 1: Write the failing parse tests**

Append to `src/cli/tests.rs`:

```rust
#[test]
fn parses_theme_subcommands() {
    assert!(matches!(parse(&["theme", "path"]).unwrap(), CliAction::ThemePath));
    assert!(matches!(parse(&["theme", "init"]).unwrap(), CliAction::ThemeInit));
    match parse(&["theme", "check"]).unwrap() {
        CliAction::ThemeCheck { path } => assert!(path.is_none()),
        other => panic!("{other:?}"),
    }
    match parse(&["theme", "check", "/tmp/t.toml"]).unwrap() {
        CliAction::ThemeCheck { path } => {
            assert_eq!(path.as_deref(), Some(std::path::Path::new("/tmp/t.toml")))
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn theme_usage_errors_are_tagged_with_the_group() {
    for args in [&["theme"][..], &["theme", "bogus"][..]] {
        match parse(args).unwrap_err() {
            Error::Usage { group, .. } => assert_eq!(group, Some("theme")),
            other => panic!("{other:?}"),
        }
    }
}
```

In `registry_matches_dispatched_groups`, add `"theme",` to the `dispatched` array.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib cli::tests::parses_theme_subcommands cli::tests::registry_matches_dispatched_groups`
Expected: compile error on the missing variants.

- [ ] **Step 3: Implement**

`src/cli/action.rs`: add after `ConfigEdit { key: String },`:

```rust
    /// Validate `~/.config/wsx/theme.toml` (or `path`) and report every error.
    ThemeCheck {
        path: Option<PathBuf>,
    },
    /// Print the resolved theme file path.
    ThemePath,
    /// Write the bundled default theme file if none exists.
    ThemeInit,
```

`src/cli/groups.rs`: add a group after the `config` group:

```rust
    GroupInfo {
        name: "theme",
        blurb: "Validate and scaffold the bar theme file (~/.config/wsx/theme.toml)",
        commands: &[
            CmdInfo {
                usage: "check [<path>]",
                blurb: "Validate the theme file and print every error",
            },
            CmdInfo {
                usage: "path",
                blurb: "Print where wsx looks for theme.toml",
            },
            CmdInfo {
                usage: "init",
                blurb: "Write the bundled default theme.toml (refuses to overwrite)",
            },
        ],
    },
```

`src/cli/parse/theme.rs`:

```rust
//! `wsx theme` — the bar theme file.

use super::Args;
use crate::cli::action::CliAction;
use crate::error::{Error, Result};
use std::path::PathBuf;

pub(in crate::cli) fn parse_theme(it: &mut Args) -> Result<CliAction> {
    match it.next().as_deref() {
        Some("check") => Ok(CliAction::ThemeCheck {
            path: it.next().map(PathBuf::from),
        }),
        Some("path") => Ok(CliAction::ThemePath),
        Some("init") => Ok(CliAction::ThemeInit),
        Some(other) => Err(Error::Usage {
            group: None,
            msg: format!("unknown theme command: {other}"),
        }),
        None => Err(Error::Usage {
            group: None,
            msg: "usage: wsx theme <check [path] | path | init>".into(),
        }),
    }
}
```

`src/cli/parse/mod.rs`: add `pub(crate) mod theme;`, `use theme::parse_theme;`, and the dispatch arm `"theme" => parse_theme(&mut it).map_err(|e| tag_group(e, group)),` next to `"config"`.

`src/cli/run.rs`: add handlers next to the `ConfigEdit` arm. `dirs` and `store` are already in scope there.

```rust
        CliAction::ThemePath => {
            println!("{}", dirs.theme_path().display());
        }
        CliAction::ThemeInit => {
            let path = dirs.theme_path();
            if path.exists() {
                return Err(Error::UserInput(format!(
                    "{} already exists; edit it in place or delete it to re-init",
                    path.display()
                )));
            }
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, crate::config::theme_file::DEFAULT_TOML)?;
            println!("wrote {}", path.display());
        }
        CliAction::ThemeCheck { path } => {
            let path = path.unwrap_or_else(|| dirs.theme_path());
            let theme_name = store.get_setting("theme")?.unwrap_or_default();
            let theme = crate::ui::theme::Theme::by_name(&theme_name);
            match crate::config::theme_file::load(&path, &theme) {
                Ok(_) if path.exists() => println!("ok: {}", path.display()),
                Ok(_) => println!(
                    "ok: no file at {}; the bundled default applies",
                    path.display()
                ),
                Err(errors) => {
                    for e in &errors {
                        eprintln!("{}: {e}", path.display());
                    }
                    return Err(Error::UserInput(format!(
                        "{} error(s) in {}",
                        errors.len(),
                        path.display()
                    )));
                }
            }
        }
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib cli::tests`
Expected: all PASS.

- [ ] **Step 5: Smoke the binary**

Run in the scratchpad (never against the real config):

```bash
XDG_CONFIG_HOME=/tmp/claude-1000/-home-eben--local-state-wsx-worktrees-workspacex-frosted-cedar/e2e1a108-5aeb-4c0f-a928-25581dd858cb/scratchpad/cfg \
XDG_STATE_HOME=/tmp/claude-1000/-home-eben--local-state-wsx-worktrees-workspacex-frosted-cedar/e2e1a108-5aeb-4c0f-a928-25581dd858cb/scratchpad/state \
sh -c 'cargo run -q -- theme path && cargo run -q -- theme init && cargo run -q -- theme check && printf "[pr]\nformat = \"\$nope\"\n" > "$XDG_CONFIG_HOME/wsx/theme.toml" && cargo run -q -- theme check; echo "exit=$?"'
```

Expected: the path, `wrote …`, `ok: …`, then one error line mentioning `nope` and `exit=1`.

- [ ] **Step 6: Commit**

```bash
git add src/cli
git commit -m "CLI: wsx theme check|path|init"
```

---

### Task 7: Dashboard footer on the engine

**Files:**
- Create: `src/ui/bar/providers.rs` (shared helpers + `keys`, `version`, `usage`)
- Create: `src/ui/bar/test_util.rs`
- Modify: `src/ui/bar/mod.rs` (declare modules; add `DashboardFooterInputs`, `dashboard_footer`, `cfg`)
- Modify: `src/app/state.rs` (`App.bar_specs`)
- Modify: `src/ui/dashboard/mod.rs` (`render_footer`, `render`)
- Modify: `src/app/render/dashboard.rs:179-188`
- Modify: `src/ui/dashboard/tests.rs` (six `render(` call sites; parity test)
- Modify: `src/ui/dashboard/layout.rs` (delete `footer` and its six tests)

**Interfaces:**
- Produces:
  - `providers::eval_segment(cfg, vars, default_style, extra: &[(&str, Style)], resolver) -> Option<Segment>`
  - `providers::eval_items(cfg, items: &[(SegmentMap, Style, Option<Hit>)], resolver) -> Option<Segment>`
  - `providers::{var, vars}` helpers; `providers::keys(cfg, items: &[(&str, &str, Option<Hit>)], resolver)`, `providers::version(cfg, version, resolver)`, `providers::usage(cfg, label, spark, resolver)`
  - `bar::cfg(specs: &BarSpecs, name: &str) -> &SegmentConfig`
  - `bar::DashboardFooterInputs { activity: &[u32], version: &str, window_label: &str, workspace_selected: bool }`
  - `bar::dashboard_footer(specs, theme, inputs, width) -> Rendered`
  - `App.bar_specs: BarSpecs` (initialized to `bundled_default(&theme)`)
  - `dashboard::render_footer(f, area, activity, theme, specs, window_label, workspace_selected) -> (Option<Rect>, Vec<(Rect, FooterHintAction)>)`
  - `test_util::{render_line, assert_lines_match, assert_rows_match, plain}`
- Consumes: Tasks 2–5; `crate::ui::footer::key_for_glyph`; `crate::ui::dashboard::sparkline::render`.

- [ ] **Step 1: Write the test helpers**

`src/ui/bar/test_util.rs`:

```rust
//! Test-only comparison helpers for the bar engine. Parity tests render a
//! legacy line and an engine line into one-row buffers and compare cells.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

pub fn plain(line: &Line<'_>) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

pub fn render_line(line: &Line<'_>, width: u16) -> Buffer {
    let mut term = Terminal::new(TestBackend::new(width, 1)).unwrap();
    term.draw(|f| f.render_widget(Paragraph::new(line.clone()), f.area()))
        .unwrap();
    term.backend().buffer().clone()
}

/// Compare row `ya` of `a` with row `yb` of `b` over `width` cells: symbol
/// and background everywhere; foreground and modifiers only where the
/// symbol is not whitespace (the grammar puts fg/bold on pad cells, which
/// is invisible on screen).
pub fn assert_rows_match(a: &Buffer, ya: u16, b: &Buffer, yb: u16, width: u16) {
    let row = |buf: &Buffer, y: u16| -> String {
        (0..width).map(|x| buf[(x, y)].symbol().to_string()).collect()
    };
    let (ta, tb) = (row(a, ya), row(b, yb));
    for x in 0..width {
        let (ca, cb) = (&a[(x, ya)], &b[(x, yb)]);
        assert_eq!(
            ca.symbol(),
            cb.symbol(),
            "symbol at col {x}\n expected: {ta:?}\n actual:   {tb:?}"
        );
        assert_eq!(ca.bg, cb.bg, "bg at col {x} ({:?})\n {ta:?}", ca.symbol());
        if ca.symbol().trim().is_empty() {
            continue;
        }
        assert_eq!(ca.fg, cb.fg, "fg at col {x} ({:?})\n {ta:?}", ca.symbol());
        assert_eq!(ca.modifier, cb.modifier, "modifier at col {x} ({:?})\n {ta:?}", ca.symbol());
    }
}

pub fn assert_lines_match(expected: &Line<'_>, actual: &Line<'_>, width: u16) {
    let (a, b) = (render_line(expected, width), render_line(actual, width));
    assert_rows_match(&a, 0, &b, 0, width);
}
```

In `src/ui/bar/mod.rs` add `pub mod providers;` and `#[cfg(test)] pub mod test_util;`.

- [ ] **Step 2: Write the providers and the footer composer**

`src/ui/bar/providers.rs`:

```rust
//! One provider per segment. A provider turns app data plus the segment's
//! `[segment]` config into a [`Segment`] by evaluating the segment's own
//! `format` against its variables, with `$style` bound to the provider's
//! state-derived default patched by the user's `style`.

use super::render::eval;
use super::segment::{Hit, Segment, SegmentConfig, SegmentMap};
use super::style::Resolver;
use ratatui::style::Style;
use ratatui::text::Span;
use std::collections::HashMap;

/// A plain variable value.
pub fn var(s: impl Into<String>) -> Segment {
    Segment::text(s, Style::default())
}

pub fn vars(entries: Vec<(&str, Segment)>) -> SegmentMap {
    entries.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

/// Evaluate `cfg.format` against `vars`. `$style` is `default_style` with
/// the user's `cfg.style` patched over it; `extra` adds more named styles
/// (`$mark_style`). `None` when the segment is disabled or renders empty.
pub fn eval_segment(
    cfg: &SegmentConfig,
    vars: &SegmentMap,
    default_style: Style,
    extra: &[(&str, Style)],
    resolver: &Resolver,
) -> Option<Segment> {
    if cfg.disabled {
        return None;
    }
    let user = resolver.resolve(&cfg.style).unwrap_or_default();
    let mut styles: HashMap<String, Style> =
        extra.iter().map(|(k, v)| (k.to_string(), *v)).collect();
    styles.insert("style".to_string(), default_style.patch(user));
    let r = resolver.with_styles(styles);
    let (seg, _) = eval(&cfg.format, vars, &r, Style::default());
    (!seg.is_empty()).then_some(seg)
}

/// Multi-item segments (`keys`, `pins`, `agents`): `cfg.format` describes
/// one item; items are joined by `cfg.separator` and each gets `hit` over
/// its own cells.
pub fn eval_items(
    cfg: &SegmentConfig,
    items: &[(SegmentMap, Style, Option<Hit>)],
    resolver: &Resolver,
) -> Option<Segment> {
    if cfg.disabled || items.is_empty() {
        return None;
    }
    let mut out = Segment::default();
    for (i, (vars, default_style, hit)) in items.iter().enumerate() {
        if i > 0 {
            out.push(Span::raw(cfg.separator.clone()));
        }
        let start = out.width;
        if let Some(seg) = eval_segment(cfg, vars, *default_style, &[], resolver) {
            out.append(seg);
        }
        if let Some(h) = hit {
            if out.width > start {
                out.hit_from(start, *h);
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

/// `(key glyph, label, hit)` per pill.
pub fn keys(cfg: &SegmentConfig, items: &[(&str, &str, Option<Hit>)], resolver: &Resolver) -> Option<Segment> {
    let items: Vec<(SegmentMap, Style, Option<Hit>)> = items
        .iter()
        .map(|(k, l, h)| (vars(vec![("key", var(*k)), ("label", var(*l))]), Style::default(), *h))
        .collect();
    eval_items(cfg, &items, resolver)
}

pub fn version(cfg: &SegmentConfig, version: &str, resolver: &Resolver) -> Option<Segment> {
    eval_segment(cfg, &vars(vec![("version", var(version))]), Style::default(), &[], resolver)
}

/// The whole segment is the usage-graph click target.
pub fn usage(cfg: &SegmentConfig, label: &str, spark: &str, resolver: &Resolver) -> Option<Segment> {
    let mut seg = eval_segment(
        cfg,
        &vars(vec![("label", var(label)), ("spark", var(spark))]),
        Style::default(),
        &[],
        resolver,
    )?;
    seg.hit_from(0, Hit::UsageGraph);
    Some(seg)
}
```

Append to `src/ui/bar/mod.rs`:

```rust
use crate::config::theme_file::BarSpecs;
use crate::ui::theme::Theme;
use render::{Rendered, render_bar};
use segment::{Hit, Segment, SegmentConfig, SegmentMap};

/// The segment config by name. Every name in `SEGMENTS` is present because
/// the user file is merged over the bundled default.
pub fn cfg<'a>(specs: &'a BarSpecs, name: &str) -> &'a SegmentConfig {
    specs
        .segments
        .get(name)
        .unwrap_or_else(|| panic!("bundled default defines [{name}]"))
}

fn put(map: &mut SegmentMap, name: &str, seg: Option<Segment>) {
    if let Some(seg) = seg {
        map.insert(name.to_string(), seg);
    }
}

pub struct DashboardFooterInputs<'a> {
    pub activity: &'a [u32],
    pub version: &'a str,
    pub window_label: &'a str,
    pub workspace_selected: bool,
}

/// The dashboard footer: key hints left, version + usage graph right.
pub fn dashboard_footer(specs: &BarSpecs, theme: &Theme, inputs: &DashboardFooterInputs<'_>, width: u16) -> Rendered {
    let resolver = specs.resolver(theme);
    let mut keys: Vec<(&str, &str)> = vec![
        ("↑↓", "nav"),
        ("↵", "open"),
        ("n", "new"),
        ("G", "group"),
        ("o", "order"),
        ("/", "filter"),
    ];
    if inputs.workspace_selected {
        keys.push(("?", "actions"));
    }
    keys.push(("q", "quit"));
    let items: Vec<(&str, &str, Option<Hit>)> = keys
        .iter()
        .map(|(k, l)| (*k, *l, crate::ui::footer::key_for_glyph(k).map(Hit::Key)))
        .collect();
    let spark = crate::ui::dashboard::sparkline::render(inputs.activity, 24);

    let mut segments = SegmentMap::new();
    put(&mut segments, "keys", providers::keys(cfg(specs, "keys"), &items, &resolver));
    put(&mut segments, "version", providers::version(cfg(specs, "version"), inputs.version, &resolver));
    put(&mut segments, "usage", providers::usage(cfg(specs, "usage"), inputs.window_label, &spark, &resolver));
    render_bar(&specs.dashboard_footer, &segments, &specs.segments, width, &resolver)
}
```

Check `sparkline` is reachable as `crate::ui::dashboard::sparkline` (it is declared in `src/ui/dashboard/mod.rs`; make the module `pub` if it is private).

- [ ] **Step 3: Write the failing parity test**

Append to `src/ui/dashboard/tests.rs`:

```rust
#[test]
fn engine_footer_matches_legacy_footer() {
    use crate::ui::bar::segment::Hit;
    use crate::ui::footer::FooterHintAction;
    let theme = Theme::wsx();
    let specs = crate::config::theme_file::bundled_default(&theme);
    let activity: Vec<u32> = (0..24).collect();
    for (selected, width) in [(true, 120u16), (false, 100u16)] {
        let (legacy, graph_w, hints) =
            layout::footer(&activity, "0.1.0", width as usize, &theme, "24h", selected);
        let new = crate::ui::bar::dashboard_footer(
            &specs,
            &theme,
            &crate::ui::bar::DashboardFooterInputs {
                activity: &activity,
                version: "0.1.0",
                window_label: "24h",
                workspace_selected: selected,
            },
            width,
        );
        crate::ui::bar::test_util::assert_lines_match(&legacy, &new.line, width);
        let new_keys: Vec<(u16, u16, FooterHintAction)> = new
            .hits
            .iter()
            .filter_map(|h| match h.hit {
                Hit::Key(k) => Some((h.start_col, h.width, FooterHintAction::Key(k))),
                Hit::ArmLeader => Some((h.start_col, h.width, FooterHintAction::ArmLeader)),
                _ => None,
            })
            .collect();
        let legacy_keys: Vec<_> = hints.iter().map(|h| (h.start_col, h.width, h.action)).collect();
        assert_eq!(new_keys, legacy_keys, "selected={selected}");
        let graph = new.hits.iter().find(|h| h.hit == Hit::UsageGraph).expect("usage hit");
        assert_eq!(graph.width, graph_w);
        assert_eq!(graph.start_col, width - graph_w);
    }
}
```

- [ ] **Step 4: Run the parity test**

Run: `cargo test --lib ui::dashboard::tests::engine_footer_matches_legacy_footer`
Expected: PASS. If a cell differs, the assertion names the column and prints both rows; fix the bundled default's `[keys]`/`[version]`/`[usage]` formats or the composer, never the legacy code.

- [ ] **Step 5: Switch the app to the engine**

`src/app/state.rs`: add the field to `App` next to `pub theme`:

```rust
    /// Bar theme (formats + palette) resolved from `~/.config/wsx/theme.toml`
    /// merged over the bundled default. Reloaded by `maybe_reload_theme`.
    pub bar_specs: crate::config::theme_file::BarSpecs,
```

and in `App::new`, after `let theme = …;`:

```rust
        let bar_specs = crate::config::theme_file::bundled_default(&theme);
```

and `bar_specs,` in the `Self { … }` initializer next to `theme,`.

`src/ui/dashboard/mod.rs`: replace `render_footer` with:

```rust
/// Render only the footer line into `area` (exactly 1 row tall) through the
/// bar engine. Returns the on-screen rect of the usage graph (when the
/// `usage` segment is present) and each clickable key hint.
pub fn render_footer(
    f: &mut Frame,
    area: Rect,
    activity: &[u32],
    theme: &Theme,
    specs: &crate::config::theme_file::BarSpecs,
    window_label: &str,
    workspace_selected: bool,
) -> (Option<Rect>, Vec<(Rect, crate::ui::footer::FooterHintAction)>) {
    use crate::ui::bar::segment::Hit;
    use crate::ui::footer::FooterHintAction;
    let rendered = crate::ui::bar::dashboard_footer(
        specs,
        theme,
        &crate::ui::bar::DashboardFooterInputs {
            activity,
            version: env!("CARGO_PKG_VERSION"),
            window_label,
            workspace_selected,
        },
        area.width,
    );
    f.render_widget(Paragraph::new(rendered.line), area);
    let mut graph = None;
    let mut hints = Vec::new();
    for (rect, hit) in crate::ui::bar::render::hit_rects(area, &rendered.hits) {
        match hit {
            Hit::UsageGraph => graph = Some(rect),
            Hit::Key(k) => hints.push((rect, FooterHintAction::Key(k))),
            Hit::ArmLeader => hints.push((rect, FooterHintAction::ArmLeader)),
            _ => {}
        }
    }
    (graph, hints)
}
```

`render` (the all-in-one used by tests, `src/ui/dashboard/mod.rs:147`): add a `specs: &crate::config::theme_file::BarSpecs` parameter after `theme` and pass it to `render_footer`. Update the six call sites in `src/ui/dashboard/tests.rs` from `render(f, f.area(), &inputs, &mut state, 0, &theme)` to `render(f, f.area(), &inputs, &mut state, 0, &theme, &specs)` with `let specs = crate::config::theme_file::bundled_default(&theme);` declared next to each `let theme = …;`.

`src/app/render/dashboard.rs:179-188`: pass `&app.bar_specs` after `&app.theme`, and change `app.usage_graph_rect = Some(graph_rect);` to `app.usage_graph_rect = graph_rect;`.

`footer_hint_rects` in `src/ui/dashboard/mod.rs:176` stays for now (the attached view still uses it until Task 9).

- [ ] **Step 6: Run the full suite**

Run: `cargo test --lib`
Expected: PASS, including `footer_row_paints_chip_bg_but_no_bar_bg`.

- [ ] **Step 7: Delete the legacy footer and port its tests**

Delete `pub fn footer` from `src/ui/dashboard/layout.rs` and the six tests that call it (`footer_offers_the_order_key`, `footer_includes_keybinds_and_sparkline`, `footer_key_pill_wraps_key_only_not_label`, `footer_uses_provided_window_label_and_reports_graph_width`, `footer_hints_align_with_rendered_key_pills`, `footer_omits_actions_pill_without_workspace`). Remove now-unused imports (`FooterHintSpan`, `FooterHintAction`, `key_for_glyph`, `sparkline`) from `layout.rs`. Replace `engine_footer_matches_legacy_footer` in `tests.rs` with a snapshot, and add the ported tests to `src/ui/bar/mod.rs`:

```rust
#[cfg(test)]
mod footer_tests {
    use super::*;
    use crate::config::theme_file::bundled_default;
    use crate::ui::bar::test_util::{plain, render_line};
    use crossterm::event::KeyCode;

    fn footer(selected: bool, label: &str, width: u16) -> Rendered {
        let theme = Theme::wsx();
        let specs = bundled_default(&theme);
        let activity: Vec<u32> = (0..24).collect();
        dashboard_footer(
            &specs,
            &theme,
            &DashboardFooterInputs { activity: &activity, version: "0.1.0", window_label: label, workspace_selected: selected },
            width,
        )
    }

    #[test]
    fn default_footer_snapshot() {
        let out = footer(true, "24h", 120);
        let text = plain(&out.line);
        assert!(
            text.starts_with(" ↑↓  nav   ↵  open   n  new   G  group   o  order   /  filter   ?  actions   q  quit"),
            "{text:?}"
        );
        let spark = crate::ui::dashboard::sparkline::render(&(0..24).collect::<Vec<u32>>(), 24);
        assert!(text.ends_with(&format!("0.1.0  24h {spark}")), "{text:?}");
        assert_eq!(out.line.width(), 120);
        assert_eq!(out.hits.iter().filter(|h| matches!(h.hit, Hit::Key(_))).count(), 8);
    }

    #[test]
    fn footer_omits_actions_pill_without_workspace() {
        assert!(!plain(&footer(false, "24h", 120).line).contains("actions"));
        assert!(plain(&footer(true, "24h", 120).line).contains("actions"));
    }

    #[test]
    fn footer_key_pill_wraps_key_only_not_label() {
        let theme = Theme::wsx();
        let out = footer(true, "24h", 120);
        let buf = render_line(&out.line, 120);
        // " ↑↓ " is cols 0..4 on the chip bg; " nav" follows on the bar bg.
        assert_eq!(buf[(1, 0)].bg, theme.bg_soft);
        assert_eq!(buf[(5, 0)].symbol(), "n");
        assert_ne!(buf[(5, 0)].bg, theme.bg_soft);
    }

    #[test]
    fn footer_hints_align_with_rendered_key_pills() {
        let out = footer(true, "24h", 120);
        let buf = render_line(&out.line, 120);
        let order = out
            .hits
            .iter()
            .find(|h| matches!(h.hit, Hit::Key(k) if k.code == KeyCode::Char('o')))
            .expect("order hint");
        let cells: String = (order.start_col..order.start_col + order.width)
            .map(|x| buf[(x, 0)].symbol().to_string())
            .collect();
        assert_eq!(cells, " o  order");
    }

    #[test]
    fn footer_usage_hit_covers_label_and_sparkline() {
        let out = footer(true, "1w", 120);
        let usage = out.hits.iter().find(|h| h.hit == Hit::UsageGraph).unwrap();
        assert_eq!(usage.width, 2 + 1 + 24);
        assert_eq!(usage.start_col + usage.width, 120);
    }
}
```

- [ ] **Step 8: Run the full suite and lints**

Run: `cargo test --lib && cargo clippy --all-targets -- -D warnings && mise exec rust@1.95.0 -- cargo fmt --all --check`
Expected: all green.

- [ ] **Step 9: Commit**

```bash
git add src/ui/bar src/ui/dashboard src/app
git commit -m "Dashboard footer: render through the bar engine, delete the legacy builder"
```

---

### Task 8: Attached top bar on the engine

**Files:**
- Modify: `src/ui/bar/providers.rs` (`agent_bar`, `workspace`, `attention`)
- Modify: `src/ui/bar/mod.rs` (`AttachedTopInputs`, `attached_top`)
- Modify: `src/ui/attached/mod.rs` (`render_panes` signature + body, `PanesDrawOutput`, delete `info_line`, port tests)
- Modify: `src/app/render/attached.rs` (both `render_panes` call sites)

**Interfaces:**
- Produces:
  - `providers::agent_bar(cfg, agent: Option<AgentKind>, theme, resolver)`, `providers::workspace(cfg, repo, name, theme, resolver)`, `providers::attention(cfg, line: Option<AttentionLine>, resolver)`
  - `bar::AttachedTopInputs { repo: &str, name: &str, agent: Option<AgentKind>, attention: Option<AttentionLine> }`, `bar::attached_top(specs, theme, inputs, width) -> Rendered`
  - `render_panes(f, panes, dividers, info_area, separator_area, chip_area, specs: &BarSpecs, repo: &str, name: &str, agent, attention: Option<AttentionLine>, pinned, procs, diff, pr, model_tokens, agents, active_agent, theme) -> PanesDrawOutput`
  - `PanesDrawOutput` gains `attention_rects: Vec<(WorkspaceId, Rect)>` and `attention_more_rect: Option<Rect>`.
- Consumes: `crate::ui::updates_bar::AttentionLine { line, segments, more }`.

- [ ] **Step 1: Add the three providers**

Append to `src/ui/bar/providers.rs`:

```rust
use crate::pty::session::AgentKind;
use crate::ui::theme::Theme;
use crate::ui::updates_bar::AttentionLine;

/// The agent identity bar; `$style` is the agent's fixed color.
pub fn agent_bar(cfg: &SegmentConfig, agent: Option<AgentKind>, theme: &Theme, resolver: &Resolver) -> Option<Segment> {
    let agent = agent?;
    let symbol = cfg.symbol.clone().unwrap_or_else(|| "▎".to_string());
    eval_segment(cfg, &vars(vec![("symbol", var(symbol))]), theme.agent_style(agent), &[], resolver)
}

/// `$repo` is absent (so `($repo/)` collapses) when the repo name is empty.
pub fn workspace(cfg: &SegmentConfig, repo: &str, name: &str, theme: &Theme, resolver: &Resolver) -> Option<Segment> {
    let mut v = vars(vec![("name", var(name))]);
    if !repo.is_empty() {
        v.insert("repo".to_string(), var(repo));
    }
    eval_segment(cfg, &v, theme.header_style(), &[], resolver)
}

/// The pre-built attention line as one opaque `$items` variable, its entry
/// and `… +N more` click extents carried as hits.
pub fn attention(cfg: &SegmentConfig, line: Option<AttentionLine>, resolver: &Resolver) -> Option<Segment> {
    let line = line?;
    let mut items = Segment::default();
    for span in line.line.spans {
        items.push(span);
    }
    for s in &line.segments {
        items.hits.push(super::segment::HitSpan { start_col: s.start_col, width: s.width, hit: Hit::Attention(s.workspace_id) });
    }
    if let Some(m) = line.more {
        items.hits.push(super::segment::HitSpan { start_col: m.start_col, width: m.width, hit: Hit::AttentionMore });
    }
    eval_segment(cfg, &vars(vec![("items", items)]), Style::default(), &[], resolver)
}
```

Append to `src/ui/bar/mod.rs`:

```rust
pub struct AttachedTopInputs<'a> {
    pub repo: &'a str,
    pub name: &'a str,
    pub agent: Option<crate::pty::session::AgentKind>,
    pub attention: Option<crate::ui::updates_bar::AttentionLine>,
}

/// The attached view's info line: agent bar, focused workspace, attention.
pub fn attached_top(specs: &BarSpecs, theme: &Theme, inputs: AttachedTopInputs<'_>, width: u16) -> Rendered {
    let resolver = specs.resolver(theme);
    let mut segments = SegmentMap::new();
    put(&mut segments, "agent_bar", providers::agent_bar(cfg(specs, "agent_bar"), inputs.agent, theme, &resolver));
    put(&mut segments, "workspace", providers::workspace(cfg(specs, "workspace"), inputs.repo, inputs.name, theme, &resolver));
    put(&mut segments, "attention", providers::attention(cfg(specs, "attention"), inputs.attention, &resolver));
    render_bar(&specs.attached_top, &segments, &specs.segments, width, &resolver)
}
```

- [ ] **Step 2: Write the failing parity test**

In `src/ui/attached/mod.rs` tests:

```rust
    #[test]
    fn engine_top_bar_matches_legacy_info_line() {
        use crate::ui::bar::segment::Hit;
        use crate::ui::updates_bar::{AttentionLine, AttentionMore, AttentionSegment};
        let theme = Theme::wsx();
        let specs = crate::config::theme_file::bundled_default(&theme);
        let attn = || AttentionLine {
            line: Line::from(vec![
                Span::styled("? foo".to_string(), theme.attention_style()),
                Span::raw("  ".to_string()),
                Span::styled("… +2 more".to_string(), theme.dim_style()),
            ]),
            segments: vec![AttentionSegment { workspace_id: crate::data::store::WorkspaceId(7), start_col: 0, width: 5 }],
            more: Some(AttentionMore { start_col: 7, width: 9 }),
        };
        for agent in [Some(AgentKind::Claude), None] {
            for with_attention in [true, false] {
                let attention = with_attention.then(attn);
                let legacy = info_line("wsx/foo", agent, attention.clone().map(|a| a.line), &theme);
                let new = crate::ui::bar::attached_top(
                    &specs,
                    &theme,
                    crate::ui::bar::AttachedTopInputs { repo: "wsx", name: "foo", agent, attention },
                    60,
                );
                crate::ui::bar::test_util::assert_lines_match(&legacy, &new.line, 60);
                if with_attention {
                    let prefix = info_line_prefix_width("wsx/foo", agent);
                    let hits: Vec<_> = new.hits.iter().map(|h| (h.start_col, h.width, h.hit)).collect();
                    assert_eq!(
                        hits,
                        vec![
                            (prefix, 5, Hit::Attention(crate::data::store::WorkspaceId(7))),
                            (prefix + 7, 9, Hit::AttentionMore),
                        ],
                        "agent={agent:?}"
                    );
                }
            }
        }
        // A label with no repo has no slash.
        let new = crate::ui::bar::attached_top(
            &specs,
            &theme,
            crate::ui::bar::AttachedTopInputs { repo: "", name: "solo", agent: None, attention: None },
            20,
        );
        assert_eq!(crate::ui::bar::test_util::plain(&new.line).trim_end(), "solo");
    }
```

- [ ] **Step 3: Run the parity test**

Run: `cargo test --lib ui::attached::tests::engine_top_bar_matches_legacy_info_line`
Expected: PASS.

- [ ] **Step 4: Switch `render_panes` to the engine**

In `src/ui/attached/mod.rs`:

- `PanesDrawOutput`: add
  ```rust
      /// `(workspace, rect)` per attention entry on the info line.
      pub attention_rects: Vec<(crate::data::store::WorkspaceId, Rect)>,
      /// Rect of the `… +N more` tail, when present.
      pub attention_more_rect: Option<Rect>,
  ```
- `render_panes`: replace the parameters `label: &str, agent: Option<AgentKind>, attention_line: Option<Line<'static>>` with `specs: &crate::config::theme_file::BarSpecs, repo: &str, name: &str, agent: Option<AgentKind>, attention: Option<crate::ui::updates_bar::AttentionLine>`. Replace the two lines `let line = info_line(label, agent, attention_line, theme); f.render_widget(Paragraph::new(line), info_area);` with:

  ```rust
      let top = crate::ui::bar::attached_top(
          specs,
          theme,
          crate::ui::bar::AttachedTopInputs { repo, name, agent, attention },
          info_area.width,
      );
      f.render_widget(Paragraph::new(top.line), info_area);
      let mut attention_rects = Vec::new();
      let mut attention_more_rect = None;
      for (rect, hit) in crate::ui::bar::render::hit_rects(info_area, &top.hits) {
          match hit {
              crate::ui::bar::segment::Hit::Attention(id) => attention_rects.push((id, rect)),
              crate::ui::bar::segment::Hit::AttentionMore => attention_more_rect = Some(rect),
              _ => {}
          }
      }
  ```
  and add `attention_rects, attention_more_rect,` to the returned `PanesDrawOutput`.
- Delete `fn info_line`. Keep `info_line_prefix_width` (the attention width budget still needs it).

In `src/app/render/attached.rs` `draw_attached`:
- Replace the `focused_label` computation with two owned strings:
  ```rust
      let (focused_repo, focused_name): (String, String) = app
          .workspaces
          .iter()
          .find(|(_, w)| w.id == focused_id)
          .map(|(_, w)| {
              let repo_name = app.repos.iter().find(|r| r.id == w.repo_id).map(|r| r.name.clone()).unwrap_or_default();
              (repo_name, w.name.clone())
          })
          .unwrap_or_default();
      let focused_label = if focused_repo.is_empty() {
          focused_name.clone()
      } else {
          format!("{focused_repo}/{focused_name}")
      };
  ```
- Delete the `attention_rects` / `attention_more_rect` / `attention_line` computations (the block from `let attention_rects: Vec<…>` through `let attention_line = attention.map(|a| a.line);`).
- In the `render_panes(` call, replace `&focused_label, focused_agent, attention_line,` with `&app.bar_specs, &focused_repo, &focused_name, focused_agent, attention,`.
- After the call, set `app.attention_rects = out.attention_rects; app.attention_more_rect = out.attention_more_rect;` instead of the locals.

In `draw_attached_remote`, replace `&label, None, None,` with `&app.bar_specs, "", &label, None, None,`.

- [ ] **Step 5: Port the tests that used `info_line`**

In `src/ui/attached/mod.rs` tests: update the `render_panes(` call in `render_panes_draws_info_on_top_and_full_width_separator` to the new signature (`&crate::config::theme_file::bundled_default(&theme), "wsx", "foo", None, None,` in place of `"wsx/foo", None, None,`). Replace `info_line_prefix_width_matches_drawn_prefix` and `info_line_label_only_when_no_attention` with:

```rust
    #[test]
    fn prefix_width_matches_drawn_prefix() {
        use crate::ui::updates_bar::AttentionLine;
        let theme = Theme::wsx();
        let specs = crate::config::theme_file::bundled_default(&theme);
        let attention = Some(AttentionLine {
            line: Line::from(vec![Span::raw("ATTN".to_string())]),
            segments: vec![],
            more: None,
        });
        let prefix = info_line_prefix_width("wsx/foo", Some(AgentKind::Claude)) as usize;
        let out = crate::ui::bar::attached_top(
            &specs,
            &theme,
            crate::ui::bar::AttachedTopInputs { repo: "wsx", name: "foo", agent: Some(AgentKind::Claude), attention },
            60,
        );
        let buf = crate::ui::bar::test_util::render_line(&out.line, 60);
        let cols: Vec<String> = (0..60).map(|x| buf[(x, 0)].symbol().to_string()).collect();
        assert_eq!(cols[prefix..prefix + 4].concat(), "ATTN", "cols={cols:?}");
    }

    #[test]
    fn top_bar_is_label_only_without_attention() {
        let theme = Theme::wsx();
        let specs = crate::config::theme_file::bundled_default(&theme);
        let out = crate::ui::bar::attached_top(
            &specs,
            &theme,
            crate::ui::bar::AttachedTopInputs { repo: "wsx", name: "foo", agent: None, attention: None },
            20,
        );
        assert_eq!(crate::ui::bar::test_util::plain(&out.line).trim_end(), "wsx/foo");
    }
```

Replace `engine_top_bar_matches_legacy_info_line` with a snapshot that keeps only its engine half: assert `plain(...)` for agent Some starts with `"▎ wsx/foo   ? foo  … +2 more"` and the two hit tuples as above.

- [ ] **Step 6: Run the suite and lints**

Run: `cargo test --lib && cargo clippy --all-targets -- -D warnings && mise exec rust@1.95.0 -- cargo fmt --all --check`
Expected: green. `draw_attached` must compile without the removed locals; `attention` is moved into `render_panes`, so nothing may use it afterwards.

- [ ] **Step 7: Commit**

```bash
git add src/ui/bar src/ui/attached src/app/render/attached.rs
git commit -m "Attached top bar: render through the bar engine, delete info_line"
```

---

### Task 9: Attached bottom bar on the engine

**Files:**
- Modify: `src/ui/bar/providers.rs` (`pins`, `agents`, `model_tokens`, `procs`, `diff`, `pr`)
- Modify: `src/ui/bar/mod.rs` (`AttachedBottomInputs`, `attached_bottom`)
- Modify: `src/ui/attached/mod.rs` (`render_panes` body; delete `key_pill_style`, `key_pill_spans`, `menu_hint_width_offsets_chips`)
- Modify: `src/ui/attached/chip_row.rs` (keep `ChipPr`, `CHIP_LABEL_COLS`; delete everything else)
- Modify: `src/ui/attached/agents_row.rs` (keep `agent_switch_keys` and its test; delete the rest)
- Modify: `src/ui/dashboard/mod.rs` (delete `footer_hint_rects` if nothing uses it after this task)

**Interfaces:**
- Produces:
  - `providers::pins(cfg, pinned: &[PinnedCommand], resolver)`, `providers::agents(cfg, agents: &[(AgentInstanceId, AgentKind, String, Option<char>)], active: Option<AgentInstanceId>, theme, resolver)`, `providers::model_tokens(cfg, mt: Option<ChipModelTokens>, theme, resolver)`, `providers::procs(cfg, procs: u32, theme, resolver)`, `providers::diff(cfg, diff: Option<DiffStats>, theme, resolver)`, `providers::pr(cfg, pr: Option<ChipPr>, theme, resolver)`
  - `bar::AttachedBottomInputs { pinned: &[PinnedCommand], procs: u32, diff: Option<DiffStats>, pr: Option<ChipPr>, model_tokens: Option<ChipModelTokens>, agents: &[(AgentInstanceId, AgentKind, String, Option<char>)], active_agent: Option<AgentInstanceId> }`, `bar::attached_bottom(specs, theme, inputs, width) -> Rendered`
- Consumes: `crate::ui::theme::{lifecycle_chip, review_mark, lifecycle_shows_review}` (all `pub(crate)`), `Theme::{lifecycle_style, review_style, status_style, agent_style, warn_style, ok_style, dim_style}`, `crate::commands::pinned::{PinnedCommand, truncate_label}`, `crate::ui::attached::chip_row::CHIP_LABEL_COLS` (make it `pub(crate)`; it already is).

- [ ] **Step 1: Add the six providers**

Append to `src/ui/bar/providers.rs`:

```rust
use crate::commands::pinned::{PinnedCommand, truncate_label};
use crate::data::store::AgentInstanceId;
use crate::git::DiffStats;
use crate::ui::attached::ChipPr;
use crate::ui::attached::chip_row::CHIP_LABEL_COLS;
use crate::ui::dashboard::status::Status;
use crate::ui::detail_modules::session_summary::ChipModelTokens;
use ratatui::style::Modifier;

/// Pinned-command chips, at most nine (they are keyed `1`–`9`).
pub fn pins(cfg: &SegmentConfig, pinned: &[PinnedCommand], resolver: &Resolver) -> Option<Segment> {
    let items: Vec<(SegmentMap, Style, Option<Hit>)> = pinned
        .iter()
        .take(9)
        .enumerate()
        .map(|(i, cmd)| {
            let label = truncate_label(&cmd.label, CHIP_LABEL_COLS);
            (
                vars(vec![("index", var((i + 1).to_string())), ("label", var(label))]),
                Style::default(),
                Some(Hit::PinnedChip(i)),
            )
        })
        .collect();
    eval_items(cfg, &items, resolver)
}

/// Agent pills: `● claude q   ○ codex w`. The active instance gets the
/// filled dot and a bold label. `$symbol` is the dot plus its space (the
/// `[agents].symbol` field is not used; the dot encodes active/idle).
pub fn agents(
    cfg: &SegmentConfig,
    agents: &[(AgentInstanceId, AgentKind, String, Option<char>)],
    active: Option<AgentInstanceId>,
    theme: &Theme,
    resolver: &Resolver,
) -> Option<Segment> {
    let items: Vec<(SegmentMap, Style, Option<Hit>)> = agents
        .iter()
        .map(|(id, kind, label, key)| {
            let is_active = active == Some(*id);
            let dot = if is_active { "● " } else { "○ " };
            let label_style = if is_active {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let mut v = vars(vec![("symbol", var(dot))]);
            v.insert("label".to_string(), Segment::text(label.clone(), label_style));
            if let Some(k) = key {
                v.insert("key".to_string(), var(k.to_string()));
            }
            (v, theme.agent_style(*kind), Some(Hit::Agent(*id)))
        })
        .collect();
    eval_items(cfg, &items, resolver)
}

/// `$style` is `ok`, or `warn` when the context window is nearly full.
pub fn model_tokens(cfg: &SegmentConfig, mt: Option<ChipModelTokens>, theme: &Theme, resolver: &Resolver) -> Option<Segment> {
    let mt = mt?;
    let style = if mt.warn { theme.warn_style() } else { theme.ok_style() };
    let mut v = vars(vec![("tokens", var(mt.tokens))]);
    if let Some(model) = mt.model {
        v.insert("model".to_string(), var(model));
    }
    eval_segment(cfg, &v, style, &[], resolver)
}

/// Hidden at zero, like the dashboard row's process dot.
pub fn procs(cfg: &SegmentConfig, procs: u32, theme: &Theme, resolver: &Resolver) -> Option<Segment> {
    if procs == 0 {
        return None;
    }
    let symbol = cfg.symbol.clone().unwrap_or_else(|| "●".to_string());
    let mut seg = eval_segment(
        cfg,
        &vars(vec![("symbol", var(symbol)), ("count", var(procs.to_string()))]),
        theme.status_style(Status::Thinking),
        &[],
        resolver,
    )?;
    seg.hit_from(0, Hit::Procs);
    Some(seg)
}

/// Hidden for a clean or unknown worktree.
pub fn diff(cfg: &SegmentConfig, diff: Option<DiffStats>, theme: &Theme, resolver: &Resolver) -> Option<Segment> {
    let d = diff?;
    if d.added == 0 && d.removed == 0 {
        return None;
    }
    eval_segment(
        cfg,
        &vars(vec![("added", var(d.added.to_string())), ("removed", var(d.removed.to_string()))]),
        theme.dim_style(),
        &[],
        resolver,
    )
}

/// `$style` is the lifecycle tint, `$mark_style` the review verdict's.
/// `$mark` is absent (so `( [$mark]($mark_style))` collapses) without a
/// verdict or on lifecycles that don't show one.
pub fn pr(cfg: &SegmentConfig, pr: Option<ChipPr>, theme: &Theme, resolver: &Resolver) -> Option<Segment> {
    use crate::ui::theme::{lifecycle_chip, lifecycle_shows_review, review_mark};
    let pr = pr?;
    let (glyph, label) = lifecycle_chip(pr.lifecycle);
    if glyph.is_empty() {
        return None;
    }
    let review = pr.review.filter(|_| lifecycle_shows_review(pr.lifecycle));
    let style = theme.lifecycle_style(Some(pr.lifecycle)).unwrap_or_else(|| theme.dim_style());
    let mark_style = review.map(|d| theme.review_style(d)).unwrap_or_default();
    let mut v = vars(vec![
        ("symbol", var(glyph)),
        ("number", var(pr.number.to_string())),
        ("label", var(label)),
    ]);
    if let Some(d) = review {
        v.insert("mark".to_string(), var(review_mark(d, pr.unresolved)));
    }
    let mut seg = eval_segment(cfg, &v, style, &[("mark_style", mark_style)], resolver)?;
    seg.hit_from(0, Hit::Pr);
    Some(seg)
}
```

`chip_row` must be reachable as `crate::ui::attached::chip_row`: change `mod chip_row;` to `pub(crate) mod chip_row;` in `src/ui/attached/mod.rs`.

`ChipModelTokens` (`src/ui/detail_modules/session_summary.rs:395`) has no derives; add `#[derive(Debug, Clone, PartialEq, Eq)]` above it, since the parity test in Step 2 uses the same value twice.

Append to `src/ui/bar/mod.rs`:

```rust
pub struct AttachedBottomInputs<'a> {
    pub pinned: &'a [crate::commands::pinned::PinnedCommand],
    pub procs: u32,
    pub diff: Option<crate::git::DiffStats>,
    pub pr: Option<crate::ui::attached::ChipPr>,
    pub model_tokens: Option<crate::ui::detail_modules::session_summary::ChipModelTokens>,
    pub agents: &'a [(
        crate::data::store::AgentInstanceId,
        crate::pty::session::AgentKind,
        String,
        Option<char>,
    )],
    pub active_agent: Option<crate::data::store::AgentInstanceId>,
}

/// The attached view's chip row: `^x menu` and pinned chips left, the
/// stats block (agents, model+tokens, procs, diff, PR) flush right.
pub fn attached_bottom(specs: &BarSpecs, theme: &Theme, inputs: AttachedBottomInputs<'_>, width: u16) -> Rendered {
    let resolver = specs.resolver(theme);
    let mut segments = SegmentMap::new();
    put(&mut segments, "keys", providers::keys(cfg(specs, "keys"), &[("^x", "menu", Some(Hit::ArmLeader))], &resolver));
    put(&mut segments, "pins", providers::pins(cfg(specs, "pins"), inputs.pinned, &resolver));
    put(&mut segments, "agents", providers::agents(cfg(specs, "agents"), inputs.agents, inputs.active_agent, theme, &resolver));
    put(&mut segments, "model_tokens", providers::model_tokens(cfg(specs, "model_tokens"), inputs.model_tokens, theme, &resolver));
    put(&mut segments, "procs", providers::procs(cfg(specs, "procs"), inputs.procs, theme, &resolver));
    put(&mut segments, "diff", providers::diff(cfg(specs, "diff"), inputs.diff, theme, &resolver));
    put(&mut segments, "pr", providers::pr(cfg(specs, "pr"), inputs.pr, theme, &resolver));
    render_bar(&specs.attached_bottom, &segments, &specs.segments, width, &resolver)
}
```

- [ ] **Step 2: Write the failing parity test**

In `src/ui/attached/mod.rs` tests. Legacy output is produced by `render_panes` (still using `render_chip_row` internally) into a frame; the new line is rendered on its own and compared cell-for-cell, then the rects.

```rust
    fn bottom_fixture() -> (
        Vec<crate::commands::pinned::PinnedCommand>,
        Option<crate::git::DiffStats>,
        Option<ChipPr>,
        Option<crate::ui::detail_modules::session_summary::ChipModelTokens>,
        Vec<(AgentInstanceId, AgentKind, String, Option<char>)>,
    ) {
        use crate::git::forge::{BranchLifecycle, ReviewDecision};
        let pinned = vec![
            crate::commands::pinned::PinnedCommand { label: "PR".into(), command: "/pr".into() },
            crate::commands::pinned::PinnedCommand { label: "feedback".into(), command: "/fb".into() },
        ];
        let diff = Some(crate::git::DiffStats { added: 12, removed: 3 });
        let pr = Some(ChipPr {
            lifecycle: BranchLifecycle::PrOpen,
            number: 42,
            review: Some(ReviewDecision::Approved),
            unresolved: Some(2),
        });
        let mt = Some(crate::ui::detail_modules::session_summary::ChipModelTokens {
            model: Some("opus 4.8".into()),
            tokens: "45k/200k".into(),
            warn: false,
        });
        let agents = vec![
            (AgentInstanceId(1), AgentKind::Claude, "claude".to_string(), Some('q')),
            (AgentInstanceId(2), AgentKind::Codex, "codex".to_string(), Some('w')),
        ];
        (pinned, diff, pr, mt, agents)
    }

    #[test]
    fn engine_bottom_bar_matches_legacy_chip_row() {
        use crate::ui::bar::segment::Hit;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let theme = Theme::wsx();
        let specs = crate::config::theme_file::bundled_default(&theme);
        let (pinned, diff, pr, mt, agents) = bottom_fixture();
        let (w, h) = (120u16, 4u16);
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        let mut legacy_out = None;
        term.draw(|f| {
            let (info, sep, _pane, chip) = layout_chrome(Rect::new(0, 0, w, h));
            legacy_out = Some(render_panes(
                f, &[], &[], info, sep, chip, &specs, "wsx", "foo", None, None,
                &pinned, 3, diff, pr, mt.clone(), &agents, Some(AgentInstanceId(1)), &theme,
            ));
        })
        .unwrap();
        let legacy_out = legacy_out.unwrap();
        let legacy_buf = term.backend().buffer().clone();

        let new = crate::ui::bar::attached_bottom(
            &specs,
            &theme,
            crate::ui::bar::AttachedBottomInputs {
                pinned: &pinned, procs: 3, diff, pr, model_tokens: mt, agents: &agents, active_agent: Some(AgentInstanceId(1)),
            },
            w,
        );
        let new_buf = crate::ui::bar::test_util::render_line(&new.line, w);
        crate::ui::bar::test_util::assert_rows_match(&legacy_buf, h - 1, &new_buf, 0, w);

        let chip_area = Rect::new(0, h - 1, w, 1);
        let rects = crate::ui::bar::render::hit_rects(chip_area, &new.hits);
        let of = |pred: &dyn Fn(Hit) -> bool| -> Vec<Rect> {
            rects.iter().filter(|(_, h)| pred(*h)).map(|(r, _)| *r).collect()
        };
        assert_eq!(of(&|h| matches!(h, Hit::PinnedChip(_))), legacy_out.chip_rects);
        assert_eq!(of(&|h| h == Hit::Pr).first().copied(), legacy_out.pr_link_rect);
        assert_eq!(of(&|h| h == Hit::Procs).first().copied(), legacy_out.procs_link_rect);
        let agent_rects: Vec<(AgentInstanceId, Rect)> = rects
            .iter()
            .filter_map(|(r, h)| match h { Hit::Agent(id) => Some((*id, *r)), _ => None })
            .collect();
        assert_eq!(agent_rects, legacy_out.agent_chip_rects);
        let leader: Vec<Rect> = of(&|h| h == Hit::ArmLeader);
        assert_eq!(leader, legacy_out.footer_hint_rects.iter().map(|(r, _)| *r).collect::<Vec<_>>());
    }
```

- [ ] **Step 3: Run the parity test**

Run: `cargo test --lib ui::attached::tests::engine_bottom_bar_matches_legacy_chip_row`
Expected: PASS. Known one-cell-class differences that are NOT acceptable here: any symbol mismatch. If the rule (`─`) length differs, check `[attached_bottom].format` ends with `($pins  )` and `right_format` starts with `(  `.

- [ ] **Step 4: Switch `render_panes` to the engine**

In `src/ui/attached/mod.rs` `render_panes`, replace everything from the `// \`^x: menu\` hint …` comment through the `render_chip_row(…)` call with:

```rust
    let bottom = crate::ui::bar::attached_bottom(
        specs,
        theme,
        crate::ui::bar::AttachedBottomInputs {
            pinned,
            procs,
            diff,
            pr,
            model_tokens,
            agents,
            active_agent,
        },
        chip_area.width,
    );
    f.render_widget(Paragraph::new(bottom.line), chip_area);
    let mut chip_rects = Vec::new();
    let mut pr_link_rect = None;
    let mut procs_link_rect = None;
    let mut agent_chip_rects = Vec::new();
    let mut footer_hint_rects = Vec::new();
    for (rect, hit) in crate::ui::bar::render::hit_rects(chip_area, &bottom.hits) {
        use crate::ui::bar::segment::Hit;
        match hit {
            Hit::PinnedChip(_) => chip_rects.push(rect),
            Hit::Pr => pr_link_rect = Some(rect),
            Hit::Procs => procs_link_rect = Some(rect),
            Hit::Agent(id) => agent_chip_rects.push((id, rect)),
            Hit::ArmLeader => footer_hint_rects.push((rect, crate::ui::footer::FooterHintAction::ArmLeader)),
            Hit::Key(k) => footer_hint_rects.push((rect, crate::ui::footer::FooterHintAction::Key(k))),
            _ => {}
        }
    }
```

and build `PanesDrawOutput { chip_rects, pr_link_rect, procs_link_rect, pane_rects, agent_chip_rects, footer_hint_rects, attention_rects, attention_more_rect }`. `chip_rects` must stay in pinned order; hits are emitted in paint order, which is pinned order.

- [ ] **Step 5: Run the suite**

Run: `cargo test --lib`
Expected: green (the parity test now compares the engine to itself through `render_panes`; it still exercises the rect mapping).

- [ ] **Step 6: Delete the legacy builders and port their tests**

- `src/ui/attached/chip_row.rs`: keep the module doc, `CHIP_LABEL_COLS`, `ChipPr` and its `#[cfg(test)] fn new`. Delete `pr_chip_parts`, `diff_chip_parts`, `procs_chip_parts`, `model_tokens_chip_parts`, `pr_chip_rect`, `layout_chip_row`, `BlockElement`, `Element`, `block_width`, `ChipRowOutput`, `render_chip_row`, and every test in the file. Remove the now-unused `use` lines.
- `src/ui/attached/agents_row.rs`: keep `agent_switch_keys` and `switch_keys_skip_reserved_and_are_unique`; delete `AGENT_PILL_GAP`, the dot consts, `agent_pill_width`, `agent_pills_width`, `agent_pills_spans`, `layout_agent_pills`, and their tests.
- `src/ui/attached/mod.rs`: delete `key_pill_style`, `key_pill_spans`, the test `menu_hint_width_offsets_chips`, and the re-export `pub(crate) use chip_row::{ChipPr, ChipRowOutput, render_chip_row};` → `pub(crate) use chip_row::ChipPr;`.
- `src/ui/dashboard/mod.rs`: delete `footer_hint_rects` if `grep -rn footer_hint_rects src/ui src/app/render` shows no remaining callers (the input handler reads `app.footer_hint_rects`, which is a field, not this fn).

Port the behavioral tests to `src/ui/bar/mod.rs`:

```rust
#[cfg(test)]
mod bottom_tests {
    use super::*;
    use crate::commands::pinned::PinnedCommand;
    use crate::config::theme_file::bundled_default;
    use crate::data::store::AgentInstanceId;
    use crate::git::forge::{BranchLifecycle, ReviewDecision};
    use crate::pty::session::AgentKind;
    use crate::ui::attached::ChipPr;
    use crate::ui::bar::test_util::{plain, render_line};
    use crate::ui::detail_modules::session_summary::ChipModelTokens;

    fn cmds(specs: &[(&str, &str)]) -> Vec<PinnedCommand> {
        specs.iter().map(|(l, c)| PinnedCommand { label: (*l).into(), command: (*c).into() }).collect()
    }
    fn pr(review: Option<ReviewDecision>) -> Option<ChipPr> {
        Some(ChipPr { lifecycle: BranchLifecycle::PrOpen, number: 42, review, unresolved: None })
    }
    fn mt() -> Option<ChipModelTokens> {
        Some(ChipModelTokens { model: Some("opus 4.8".into()), tokens: "45k/200k".into(), warn: false })
    }
    fn agents() -> Vec<(AgentInstanceId, AgentKind, String, Option<char>)> {
        vec![
            (AgentInstanceId(1), AgentKind::Claude, "claude".into(), Some('q')),
            (AgentInstanceId(2), AgentKind::Codex, "codex".into(), Some('w')),
        ]
    }
    fn render(inputs: AttachedBottomInputs<'_>, width: u16) -> Rendered {
        let theme = Theme::wsx();
        let specs = bundled_default(&theme);
        attached_bottom(&specs, &theme, inputs, width)
    }
    fn full<'a>(pinned: &'a [PinnedCommand], agents: &'a [(AgentInstanceId, AgentKind, String, Option<char>)]) -> AttachedBottomInputs<'a> {
        AttachedBottomInputs {
            pinned,
            procs: 3,
            diff: Some(crate::git::DiffStats { added: 12, removed: 3 }),
            pr: pr(Some(ReviewDecision::Approved)),
            model_tokens: mt(),
            agents,
            active_agent: Some(AgentInstanceId(1)),
        }
    }
    fn present(out: &Rendered) -> Vec<&'static str> {
        let t = plain(&out.line);
        let mut v = Vec::new();
        if t.contains("claude") { v.push("agents"); }
        if t.contains("45k/200k") { v.push("model_tokens"); }
        if t.contains("3p") { v.push("procs"); }
        if t.contains("+12") { v.push("diff"); }
        if t.contains("#42") { v.push("pr"); }
        v
    }

    #[test]
    fn default_bottom_snapshot_and_hits() {
        let pinned = cmds(&[("PR", "/pr"), ("feedback", "/fb")]);
        let agents = agents();
        let out = render(full(&pinned, &agents), 120);
        let t = plain(&out.line);
        // keys ` ^x  menu`, two literal spaces, chips ` 1  PR` and ` 2  feedback`
        // (pill pad + `index` pad + space-led label), two spaces, then the rule.
        assert!(t.starts_with(" ^x  menu   1  PR   2  feedback  ──"), "{t:?}");
        // Each agent pill ends with its ` q ` key pill (trailing pad), then the
        // 3-cell separator / group gap, so four spaces precede `○` and `opus`.
        assert!(t.ends_with("● claude  q    ○ codex  w    opus 4.8 45k/200k ● 3p +12 −3 ⏺ #42 open ✓"), "{t:?}");
        assert_eq!(out.line.width(), 120);
        let chips: Vec<_> = out.hits.iter().filter(|h| matches!(h.hit, Hit::PinnedChip(_))).collect();
        assert_eq!(chips.len(), 2);
        assert_eq!(chips[0].width, 6, "` 1  PR` is 6 cells");
        assert_eq!(chips[1].width, 12, "` 2  feedback` is 12 cells");
        assert_eq!(chips[1].start_col, chips[0].start_col + 6 + 2, "2-cell gap");
        let leader = out.hits.iter().find(|h| h.hit == Hit::ArmLeader).unwrap();
        assert_eq!((leader.start_col, leader.width), (0, 9));
        let pr = out.hits.iter().find(|h| h.hit == Hit::Pr).unwrap();
        assert_eq!(pr.start_col + pr.width, 120, "PR chip hugs the right edge");
        let ids: Vec<_> = out.hits.iter().filter_map(|h| match h.hit { Hit::Agent(id) => Some(id), _ => None }).collect();
        assert_eq!(ids, vec![AgentInstanceId(1), AgentInstanceId(2)]);
    }

    #[test]
    fn zero_procs_and_clean_diff_render_nothing() {
        let pinned = cmds(&[]);
        let agents = vec![];
        let mut inputs = full(&pinned, &agents);
        inputs.procs = 0;
        inputs.diff = Some(crate::git::DiffStats { added: 0, removed: 0 });
        let out = render(inputs, 120);
        assert_eq!(present(&out), vec!["model_tokens", "pr"]);
        assert!(out.hits.iter().all(|h| h.hit != Hit::Procs));
    }

    #[test]
    fn pr_mark_is_absent_without_a_verdict() {
        let pinned = cmds(&[]);
        let agents = vec![];
        let mut inputs = full(&pinned, &agents);
        inputs.pr = pr(None);
        let t = plain(&render(inputs, 120).line);
        assert!(t.ends_with("⏺ #42 open"), "{t:?}");
    }

    #[test]
    fn narrow_rows_drop_model_tokens_then_agents_then_procs_then_diff() {
        let pinned = cmds(&[("PR", "/pr")]);
        let agents = agents();
        // Left side is 19 cells (` ^x  menu` + 2 + ` 1  PR` + 2). Right side at
        // full strength is 73: 2 + agents 26 + 3 + model 17 + 1 + procs 4 + 1
        // + diff 6 + 1 + pr 12. Dropping model removes 18, agents 29, procs 5,
        // diff 7.
        let widths_and_expect: [(u16, &[&str]); 5] = [
            (120, &["agents", "model_tokens", "procs", "diff", "pr"]),
            (80, &["agents", "procs", "diff", "pr"]),
            (50, &["procs", "diff", "pr"]),
            (40, &["diff", "pr"]),
            (35, &["pr"]),
        ];
        for (w, expect) in widths_and_expect {
            let out = render(full(&pinned, &agents), w);
            assert_eq!(present(&out), expect.to_vec(), "width {w}: {:?}", plain(&out.line));
        }
    }

    #[test]
    fn model_tokens_warn_style_is_the_warn_color() {
        let theme = Theme::wsx();
        let pinned = cmds(&[]);
        let agents = vec![];
        let mut inputs = full(&pinned, &agents);
        inputs.model_tokens = Some(ChipModelTokens { model: None, tokens: "190k/200k".into(), warn: true });
        inputs.pr = None;
        inputs.diff = None;
        inputs.procs = 0;
        let out = render(inputs, 60);
        let buf = render_line(&out.line, 60);
        assert_eq!(buf[(59, 0)].symbol(), "k");
        assert_eq!(buf[(59, 0)].fg, theme.warn);
    }
}
```

The widths in `narrow_rows_drop_…` are derived from the fixture's element widths (see the comment in the test). If one boundary is off by a cell, adjust that width by one rather than the expectation order.

- [ ] **Step 7: Run the suite and lints**

Run: `cargo test --lib && cargo clippy --all-targets -- -D warnings && mise exec rust@1.95.0 -- cargo fmt --all --check`
Expected: green.

- [ ] **Step 8: Commit**

```bash
git add src/ui
git commit -m "Attached chip row: render through the bar engine, delete the legacy builders"
```

---

### Task 10: Live reload and the error notice

**Files:**
- Create: `src/app/theme_reload.rs`
- Modify: `src/app/mod.rs` (declare the module)
- Modify: `src/app/state.rs` (three fields + initializers)
- Modify: `src/app/run.rs` (tick arm)
- Modify: `src/main.rs` (set the path)
- Modify: `src/ui/dashboard/mod.rs` (`render_footer` notice) and `src/app/render/dashboard.rs` (pass it)

**Interfaces:**
- Produces: `App.theme_path: Option<PathBuf>`, `App.theme_fingerprint: Option<(SystemTime, u64)>`, `App.theme_notice: Option<(String, u64)>`; `App::set_theme_path(&mut self, path: PathBuf, now_ms: u64)`, `App::maybe_reload_theme(&mut self, now_ms: u64)`, `App::reload_theme(&mut self, now_ms: u64)`, `App::theme_notice(&self, now_ms: u64) -> Option<&str>`; `render_footer(..., notice: Option<&str>)`.
- Consumes: `theme_file::load` (Task 5), `App.bar_specs` (Task 7), `App.tick` (existing), `crate::util::time::now_ms_u64()`.

- [ ] **Step 1: Write the failing tests**

`src/app/theme_reload.rs`:

```rust
//! Reload `theme.toml` while wsx runs: a fingerprint (mtime + length)
//! check once a second on the housekeeping tick, last-good specs kept on
//! error, and a short footer notice naming the first problem.

use super::App;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Ticks between fingerprint checks: 8 × 125 ms = 1 s. (`App.tick` is `u32`.)
const CHECK_EVERY_TICKS: u32 = 8;
/// How long the footer shows a theme error.
const NOTICE_MS: u64 = 5_000;

fn fingerprint(path: &Path) -> Option<(SystemTime, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

impl App {
    /// Point the app at its theme file and load it now.
    pub fn set_theme_path(&mut self, path: PathBuf, now_ms: u64) {
        self.theme_fingerprint = fingerprint(&path);
        self.theme_path = Some(path);
        self.reload_theme(now_ms);
    }

    /// Called every tick; does the fingerprint check once a second and
    /// reloads only when the file changed (or appeared / disappeared).
    pub fn maybe_reload_theme(&mut self, now_ms: u64) {
        if self.tick % CHECK_EVERY_TICKS != 0 {
            return;
        }
        let Some(path) = self.theme_path.as_deref() else {
            return;
        };
        let fp = fingerprint(path);
        if fp == self.theme_fingerprint {
            return;
        }
        self.theme_fingerprint = fp;
        self.reload_theme(now_ms);
    }

    /// Load the file unconditionally. Errors keep the last good specs.
    pub fn reload_theme(&mut self, now_ms: u64) {
        let Some(path) = self.theme_path.clone() else {
            return;
        };
        match crate::config::theme_file::load(&path, &self.theme) {
            Ok(specs) => {
                self.bar_specs = specs;
                self.theme_notice = None;
                tracing::info!(path = %path.display(), "theme.toml loaded");
            }
            Err(errors) => {
                for e in &errors {
                    tracing::warn!(path = %path.display(), "theme.toml: {e}");
                }
                let first = &errors[0];
                let msg = if errors.len() > 1 {
                    format!("theme.toml: {first} (+{} more)", errors.len() - 1)
                } else {
                    format!("theme.toml: {first}")
                };
                self.theme_notice = Some((msg, now_ms + NOTICE_MS));
            }
        }
    }

    /// The notice to show in the footer, if one is still live.
    pub fn theme_notice(&self, now_ms: u64) -> Option<&str> {
        self.theme_notice
            .as_ref()
            .filter(|(_, until)| now_ms < *until)
            .map(|(m, _)| m.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::store::Store;
    use crate::ui::bar::format;

    fn app() -> App {
        App::new(Store::open_in_memory().unwrap(), PathBuf::from("/tmp/wsx-theme-reload-test")).unwrap()
    }

    #[test]
    fn missing_file_is_the_bundled_default_without_notice() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app();
        app.set_theme_path(dir.path().join("theme.toml"), 0);
        assert_eq!(app.bar_specs.dashboard_footer.format, format::parse("$keys").unwrap());
        assert!(app.theme_notice(0).is_none());
    }

    #[test]
    fn invalid_edit_keeps_last_good_and_sets_a_timed_notice() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("theme.toml");
        std::fs::write(&path, "[dashboard_footer]\nformat = \"$version\"\n").unwrap();
        let mut app = app();
        app.set_theme_path(path.clone(), 0);
        assert_eq!(app.bar_specs.dashboard_footer.format, format::parse("$version").unwrap());

        std::fs::write(&path, "[dashboard_footer]\nformat = \"$nope\"\n").unwrap();
        app.reload_theme(1_000);
        assert_eq!(app.bar_specs.dashboard_footer.format, format::parse("$version").unwrap(), "last good kept");
        let notice = app.theme_notice(1_000).expect("notice set");
        assert!(notice.starts_with("theme.toml: [dashboard_footer].format"), "{notice}");
        assert!(notice.contains("nope"), "{notice}");
        assert!(app.theme_notice(5_999).is_some());
        assert!(app.theme_notice(6_000).is_none(), "expires after 5 s");

        std::fs::write(&path, "[dashboard_footer]\nformat = \"$usage\"\n").unwrap();
        app.reload_theme(7_000);
        assert_eq!(app.bar_specs.dashboard_footer.format, format::parse("$usage").unwrap());
        assert!(app.theme_notice(7_000).is_none(), "fixed file clears the notice");
    }

    #[test]
    fn tick_check_reloads_only_on_a_changed_fingerprint_once_a_second() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("theme.toml");
        std::fs::write(&path, "[dashboard_footer]\nformat = \"$version\"\n").unwrap();
        let mut app = app();
        app.set_theme_path(path.clone(), 0);

        // Different length guarantees a different fingerprint even within
        // the same mtime granularity.
        std::fs::write(&path, "[dashboard_footer]\nformat = \"$version  $usage\"\n").unwrap();
        app.tick = 3;
        app.maybe_reload_theme(0);
        assert_eq!(app.bar_specs.dashboard_footer.format, format::parse("$version").unwrap(), "off-tick: no check");
        app.tick = 8;
        app.maybe_reload_theme(0);
        assert_eq!(app.bar_specs.dashboard_footer.format, format::parse("$version  $usage").unwrap());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib app::theme_reload`
Expected: compile errors (module not declared, fields missing).

- [ ] **Step 3: Wire the fields, the tick, and main**

`src/app/mod.rs`: add `pub mod theme_reload;` next to the other `pub mod` lines.

`src/app/state.rs`: add to `App` next to `bar_specs`:

```rust
    /// `~/.config/wsx/theme.toml`, set by `main` after construction. `None`
    /// in tests, which then keep the bundled default.
    pub theme_path: Option<std::path::PathBuf>,
    /// `(mtime, len)` of the theme file at the last check; `None` when absent.
    pub theme_fingerprint: Option<(std::time::SystemTime, u64)>,
    /// `(message, expires_at_ms)` for the footer after a failed reload.
    pub theme_notice: Option<(String, u64)>,
```

and initialize `theme_path: None, theme_fingerprint: None, theme_notice: None,` in `App::new`.

`src/app/run.rs`, in the `_ = tick.tick() =>` arm right after the `reply_draft_clear_at_ms` block:

```rust
                // Pick up edits to ~/.config/wsx/theme.toml (fingerprint
                // check once a second; reparse only on change).
                g.maybe_reload_theme(now_ms);
```

`src/main.rs`: replace `let app = Arc::new(Mutex::new(app::App::new(store, worktree_base)?));` with:

```rust
    let mut app_state = app::App::new(store, worktree_base)?;
    app_state.set_theme_path(dirs.theme_path(), util::time::now_ms_u64());
    let app = Arc::new(Mutex::new(app_state));
```

(use the crate path `main.rs` already uses for `util`; if `util` is not imported there, write `wsx::util::time::now_ms_u64()` to match how `app::App` is referenced.)

- [ ] **Step 4: Show the notice in the footer**

`src/ui/dashboard/mod.rs` `render_footer`: add a final parameter `notice: Option<&str>` and, as the first statement:

```rust
    if let Some(msg) = notice {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(msg.to_string(), theme.err_style()))),
            area,
        );
        return (None, Vec::new());
    }
```

Update `render` (the test entry point) to pass `None`, and `src/app/render/dashboard.rs` to pass `app.theme_notice(crate::util::time::now_ms_u64())`. Note `app` is `&mut App` there; bind the notice to an owned `Option<String>` before the call (`let notice = app.theme_notice(now).map(str::to_string);`) so the borrow doesn't overlap the later `app.usage_graph_rect = …` writes.

Add to `src/ui/dashboard/tests.rs`:

```rust
#[test]
fn footer_shows_a_theme_notice_instead_of_hints() {
    let theme = Theme::wsx();
    let specs = crate::config::theme_file::bundled_default(&theme);
    let backend = TestBackend::new(80, 1);
    let mut term = Terminal::new(backend).unwrap();
    let mut out = None;
    term.draw(|f| {
        out = Some(render_footer(f, f.area(), &[], &theme, &specs, "24h", true, Some("theme.toml: [pr].format: col 3: unknown `$nope`")));
    })
    .unwrap();
    let (graph, hints) = out.unwrap();
    assert!(graph.is_none());
    assert!(hints.is_empty());
    let buf = term.backend().buffer();
    let row: String = (0..80).map(|x| buf[(x, 0)].symbol().to_string()).collect();
    assert!(row.starts_with("theme.toml: [pr].format"), "{row:?}");
    assert_eq!(buf[(0, 0)].fg, theme.err);
}
```

- [ ] **Step 5: Run the suite and lints**

Run: `cargo test --lib && cargo clippy --all-targets -- -D warnings && mise exec rust@1.95.0 -- cargo fmt --all --check`
Expected: green.

- [ ] **Step 6: Try it live**

Run wsx with the scratchpad config dir from Task 6 Step 5 (`XDG_CONFIG_HOME=… XDG_STATE_HOME=… cargo run`), then from another terminal edit that `theme.toml`: set `[attached_top] format = "[](fg:#3a3a3a)[ $workspace ](bg:#3a3a3a fg:#d75f00)[](fg:#3a3a3a)( $attention)"`, save, and confirm the attached top bar changes within a second. Introduce `$nope`, save, confirm the dashboard footer shows the red notice for ~5 s and the bars keep the previous look.

- [ ] **Step 7: Commit**

```bash
git add src/app src/main.rs src/ui/dashboard
git commit -m "Theme: reload theme.toml on change, footer notice on errors"
```

---

### Task 11: Documentation, manual test, and spec sync

**Files:**
- Modify: `docs/book/src/configuration/themes.md`
- Modify: `docs/book/src/configuration/global-settings.md` (the `theme` row)
- Modify: `docs/book/src/reference/storage-and-config-files.md` (add `theme.toml`)
- Create: `docs/manual-tests/bar-theming.md`
- Modify: `docs/superpowers/specs/2026-09-13-bar-theming-design.md` (segment table deviations)

- [ ] **Step 1: Rewrite the themes page**

Replace `docs/book/src/configuration/themes.md` with:

````markdown
# Themes

wsx has two layers of theming: a **base palette** chosen with the `theme`
setting, and a **bar theme file** that describes what the dashboard footer
and the attached view's top and bottom bars contain and how each piece is
styled, using starship's format grammar.

## Base palette

```
wsx config set theme dracula
wsx config set theme jellybeans
wsx config set theme nord
wsx config set theme wsx        # default
wsx config set theme default    # ANSI colors that follow your terminal
```

The base palette colors repo headers, the selected row, status dots, modals,
and markdown. Restart wsx after changing it. Its colors are also available
to the bar theme file as *theme tokens* (below).

## Bar theme file

```
wsx theme path     # where wsx looks: ~/.config/wsx/theme.toml
wsx theme init     # write the bundled default there (never overwrites)
wsx theme check    # validate and print every error; exit 1 on any
```

The file is optional. Anything you leave out keeps the bundled default,
which reproduces wsx's stock bars exactly, so `wsx theme init` gives you a
fully commented starting point. wsx re-reads the file within a second of
every save. If a save has an error, the bars keep their last good look, the
dashboard footer shows the first error in red for five seconds, and the
full list goes to the log.

### Bars

```toml
[dashboard_footer]
format       = "$keys"
right_format = "$version  $usage"

[attached_top]
format = "($agent_bar )$workspace(   $attention)"

[attached_bottom]
format       = "$keys  ($pins  )"
right_format = "(  ($agents   )($model_tokens )($procs )($diff )$pr)"
fill         = "─"
fill_style   = "fg:dim"
```

| Key | Meaning |
|---|---|
| `format` | The left side. Never dropped; clipped at the right edge if too long. |
| `right_format` | Flush right. When the bar is too narrow, segments are removed lowest `priority` first until it fits. |
| `style` | Base style for literal text and the fill; everything inherits it. |
| `fill`, `fill_style` | The character (first char) repeated across the gap between the two sides. |

### Grammar

| Syntax | Meaning |
|---|---|
| `$name`, `${name}` | Insert a segment (in a `[segment]` table: one of its variables). |
| `[text](style)` | Style a run. Inner runs inherit the outer background unless they set their own, so powerline blocks compose. |
| `( … )` | Render only if a `$name` inside produced output. Put separators inside the group so they vanish with the segment. |
| `$$`, `\[`, `\(`, `\x` | Literal characters. |

A **style** is space-separated tokens: `fg:<c>`, `bg:<c>`, a bare `<c>`
(foreground), `bold`, `dimmed`, `italic`, `underline`, `none`, and `$style`
(the segment's own style, see below). A **color** is `#rrggbb`, a 0–255
index, an ANSI name (`red`, `bright-blue`, `white`), a `[palette]` name, or
a theme token: `dim path code bg_alt bg_soft ok warn err attention merged
header_fg selected_fg selected_bg question stalled waiting thinking complete
idle brand`. Palette names shadow theme tokens, which shadow ANSI names.

### Segments

Each segment has a `[segment]` table with `format` (its own layout, using
the variables below), `style` (merged over the segment's state color and
exposed as `$style`), `symbol`, `disabled`, `priority` (overflow survival;
higher lasts longer), and, for multi-item segments, `separator`.

| Segment | Variables | Notes |
|---|---|---|
| `keys` | `$key $label` | One pill per key hint. Clickable. |
| `version` | `$version` | |
| `usage` | `$label $spark` | The activity sparkline. Clickable. |
| `agent_bar` | `$symbol` | `$style` is the agent's identity color. Attached only. |
| `workspace` | `$repo $name` | `$repo` is absent when there is no repo name. |
| `attention` | `$items` | Cross-workspace attention list. Clickable. |
| `pins` | `$index $label` | One chip per pinned command. Clickable. |
| `agents` | `$symbol $label $key` | One pill per agent (2+ agents). `$style` is the agent color. Clickable. |
| `model_tokens` | `$model $tokens` | `$style` is `ok`, or `warn` near the context limit. |
| `procs` | `$symbol $count` | Hidden at zero. Clickable. |
| `diff` | `$added $removed` | Hidden when clean. |
| `pr` | `$symbol $number $label $mark` | `$style` is the lifecycle tint, `$mark_style` the review verdict. Clickable. |

Segments that don't apply to a bar (everything but `keys`, `version`, and
`usage` on the dashboard) render empty there, so one file works for all
three bars.

### A powerline example

```toml
[palette]
first  = "#121212"
second = "#3a3a3a"
rust   = "#d75f00"

[attached_top]
format = "[](fg:first)[ $workspace ](bg:first fg:rust)[](fg:first bg:second)( $attention )[](fg:second)"

[attached_bottom]
right_format = "(  [](fg:second)[ $agents ](bg:second)([](fg:first bg:second)[ $pr ](bg:first)))"

[workspace]
style = "bold"

[pr]
style = "fg:rust"
````

- [ ] **Step 2: Update the settings row and the storage reference**

In `docs/book/src/configuration/global-settings.md`, change the `theme` row's text to: `Base color palette. One of `wsx` (default), `default` (palette-adaptive ANSI), `dracula`, `jellybeans`, `nord`. Unknown values fall back to `wsx`. Restart wsx after changing. Bar layout and styling live in `~/.config/wsx/theme.toml`; see [Themes](themes.md).`

In `docs/book/src/reference/storage-and-config-files.md`, add a row or bullet: `~/.config/wsx/theme.toml` (honors `XDG_CONFIG_HOME`) — optional bar theme file; see Themes. Reloaded while running.

- [ ] **Step 3: Write the manual test**

`docs/manual-tests/bar-theming.md`:

```markdown
# Manual test — starship-style bar theming

Spec: `docs/superpowers/specs/2026-09-13-bar-theming-design.md`

Run everything against a scratch config so your real file is untouched:

```bash
export XDG_CONFIG_HOME=/tmp/wsx-theme-test/config XDG_STATE_HOME=/tmp/wsx-theme-test/state
```

## 1. Stock look with no file

Start `wsx` with at least one repo and a workspace. Expected: the dashboard
footer and, after attaching, the top and bottom bars look exactly as before.

## 2. Init and check

```bash
wsx theme path     # prints $XDG_CONFIG_HOME/wsx/theme.toml
wsx theme init     # writes it; a second run refuses
wsx theme check    # ok: <path>
```

## 3. Powerline top bar, live

With wsx attached to a workspace, edit the file in another terminal:

```toml
[attached_top]
format = "[](fg:#3a3a3a)[ $workspace ](bg:#3a3a3a fg:#d75f00)[](fg:#3a3a3a)( $attention)"
```

Expected within one second: the top bar shows the workspace name on a grey
block with rounded caps; the attention items follow only when present.

## 4. Overflow order

Set `right_format` on `[attached_bottom]` to the default and resize the
terminal narrower step by step. Expected drop order: model + tokens first,
then agent pills, then the process count, then the diff, with the PR chip
surviving longest.

## 5. Broken edit

Change a segment name to `$nope` and save. Expected: the bars keep their
previous look; the dashboard footer shows
`theme.toml: [attached_top].format: unknown `$nope` …` in red for about five
seconds; `wsx theme check` exits 1 and prints the same error. Fix the file:
the bars update and the notice clears.

## 6. Clicks follow segments

Move `$pr` from `right_format` to the start of `[attached_bottom].format`.
Expected: the PR chip appears at the left and clicking it still opens the
PR; clicking `^x menu` still opens the leader menu.
```

- [ ] **Step 4: Sync the spec's segment table**

In `docs/superpowers/specs/2026-09-13-bar-theming-design.md`, update the Segments table rows for `agents` (`$symbol $label $key`), `model_tokens` (`$model $tokens`), `pr` (`$symbol $number $label $mark`, `$mark_style`), and `attention` (`$items`), and add `fill`/`fill_style` to the `[bar]` keys in the Grammar section, so the spec matches what shipped.

- [ ] **Step 5: Build the book if mdbook is installed**

Run: `command -v mdbook && (cd docs/book && mdbook build)`
Expected: builds without warnings, or `mdbook` is absent (skip).

- [ ] **Step 6: Commit**

```bash
git add docs
git commit -m "Docs: bar theme file, wsx theme commands, manual test"
```

---

## Self-review notes

- Spec coverage: file location and merge (Tasks 1, 5); grammar (2, 3); segments and hits (4, 7–9); overflow priority (4, 9); load/reload/errors and CLI (5, 6, 10); parity snapshots and docs (7–9, 11). The spec's commit order put the loader first; this plan builds the parsers first because the loader validates with them.
- Known threshold difference: the legacy chip row dropped elements when `chips + 2 + block` exceeded the row; the engine drops when `left + right` does, and the default `format` ends in two literal spaces after the pins, so the engine drops two cells earlier than legacy at the exact boundary. Invisible in practice; parity tests use widths with slack.
- Known behavior change: a pinned chip partially past the right edge used to be dropped entirely; the engine clips it and keeps the visible part clickable.
