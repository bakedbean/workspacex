//! Basic, dependency-free syntax highlighting for the chronology detail modal.
//! A single generic tokenizer driven by a per-language `LangSpec`. Per-line,
//! no cross-line state — "basic" fidelity for a glanceable diff.

use crate::activity::chronology::ChangeDetail;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::path::Path;

/// Minimal language description driving the generic tokenizer.
pub struct LangSpec {
    pub keywords: &'static [&'static str],
    pub line_comment: &'static [&'static str],
    pub string_delims: &'static [char],
}

static RUST: LangSpec = LangSpec {
    keywords: &[
        "fn", "let", "mut", "pub", "use", "struct", "enum", "impl", "trait", "for", "in", "if",
        "else", "match", "while", "loop", "return", "self", "Self", "mod", "const", "static",
        "move", "ref", "as", "where", "async", "await", "dyn", "crate", "super", "type", "unsafe",
        "break", "continue", "true", "false",
    ],
    line_comment: &["//"],
    string_delims: &['"'],
};

static CLIKE: LangSpec = LangSpec {
    keywords: &[
        "if",
        "else",
        "for",
        "while",
        "switch",
        "case",
        "break",
        "continue",
        "return",
        "struct",
        "class",
        "const",
        "static",
        "void",
        "int",
        "char",
        "bool",
        "new",
        "delete",
        "public",
        "private",
        "protected",
        "function",
        "var",
        "let",
        "import",
        "export",
        "from",
        "default",
        "true",
        "false",
        "null",
    ],
    line_comment: &["//"],
    string_delims: &['"', '\''],
};

static PYTHON: LangSpec = LangSpec {
    keywords: &[
        "def", "class", "return", "if", "elif", "else", "for", "while", "import", "from", "as",
        "with", "try", "except", "finally", "raise", "lambda", "None", "True", "False", "and",
        "or", "not", "in", "is", "pass", "yield", "global", "nonlocal",
    ],
    line_comment: &["#"],
    string_delims: &['"', '\''],
};

static SHELL: LangSpec = LangSpec {
    keywords: &[
        "if", "then", "else", "elif", "fi", "for", "in", "do", "done", "while", "case", "esac",
        "function", "return", "export", "local",
    ],
    line_comment: &["#"],
    string_delims: &['"', '\''],
};

/// Pick a `LangSpec` from a path's extension; `None` → no highlighting.
pub fn lang_for_path(path: &Path) -> Option<&'static LangSpec> {
    let ext = path.extension().and_then(|e| e.to_str())?;
    match ext {
        "rs" => Some(&RUST),
        "c" | "h" | "cc" | "cpp" | "cxx" | "hpp" | "hh" | "js" | "jsx" | "ts" | "tsx" | "go"
        | "java" | "cs" | "json" => Some(&CLIKE),
        "py" => Some(&PYTHON),
        "sh" | "bash" | "zsh" => Some(&SHELL),
        _ => None,
    }
}

fn kw_style() -> Style {
    Style::default().fg(Color::Magenta)
}
fn str_style() -> Style {
    Style::default().fg(Color::Yellow)
}
fn comment_style() -> Style {
    Style::default().fg(Color::DarkGray)
}
fn num_style() -> Style {
    Style::default().fg(Color::Cyan)
}

fn flush(spans: &mut Vec<Span<'static>>, default: &mut String) {
    if !default.is_empty() {
        spans.push(Span::raw(std::mem::take(default)));
    }
}

fn take_while(rest: &str, pred: impl Fn(char) -> bool) -> (String, usize) {
    let mut tok = String::new();
    let mut bytes = 0;
    for c in rest.chars() {
        if pred(c) {
            tok.push(c);
            bytes += c.len_utf8();
        } else {
            break;
        }
    }
    (tok, bytes)
}

fn take_string(rest: &str, delim: char) -> (String, usize) {
    let mut tok = String::new();
    let mut bytes = 0;
    let mut chars = rest.chars();
    let open = chars.next().unwrap();
    tok.push(open);
    bytes += open.len_utf8();
    let mut escaped = false;
    for c in chars {
        tok.push(c);
        bytes += c.len_utf8();
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        if c == delim {
            break;
        }
    }
    (tok, bytes)
}

/// Tokenize ONE line of code into styled spans by `spec`. Priority: line
/// comment (rest of line) > string > number > keyword/identifier > default.
pub fn highlight_code(text: &str, spec: &LangSpec) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut default = String::new();
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        if spec.line_comment.iter().any(|c| rest.starts_with(c)) {
            flush(&mut spans, &mut default);
            spans.push(Span::styled(rest.to_string(), comment_style()));
            return spans;
        }
        let ch = rest.chars().next().unwrap();
        if spec.string_delims.contains(&ch) {
            flush(&mut spans, &mut default);
            let (tok, consumed) = take_string(rest, ch);
            spans.push(Span::styled(tok, str_style()));
            i += consumed;
        } else if ch.is_ascii_digit() {
            flush(&mut spans, &mut default);
            let (tok, consumed) = take_while(rest, |c| c.is_ascii_digit() || c == '.' || c == '_');
            spans.push(Span::styled(tok, num_style()));
            i += consumed;
        } else if ch.is_alphabetic() || ch == '_' {
            let (tok, consumed) = take_while(rest, |c| c.is_alphanumeric() || c == '_');
            if spec.keywords.contains(&tok.as_str()) {
                flush(&mut spans, &mut default);
                spans.push(Span::styled(tok, kw_style()));
            } else {
                default.push_str(&tok);
            }
            i += consumed;
        } else {
            default.push(ch);
            i += ch.len_utf8();
        }
    }
    flush(&mut spans, &mut default);
    spans
}

fn code_spans(code: &str, lang: Option<&LangSpec>) -> Vec<Span<'static>> {
    match lang {
        Some(spec) => highlight_code(code, spec),
        None => vec![Span::raw(code.to_string())],
    }
}

/// Build the modal's styled diff lines: dim 4-col gutter, green `+` / red `-`
/// marker, then highlighted code (or a plain span when `lang` is None). Added
/// lines numbered from `base_line`; removed lines blank gutter. No line cap.
pub fn change_detail_lines_styled(
    detail: &ChangeDetail,
    base_line: u32,
    lang: Option<&LangSpec>,
) -> Vec<Line<'static>> {
    let dim = Style::default().add_modifier(Modifier::DIM);
    let add = Style::default().fg(Color::Green);
    let del = Style::default().fg(Color::Red);
    let mut out = Vec::new();
    let push = |gutter: String,
                marker_style: Style,
                marker: &str,
                code: &str,
                out: &mut Vec<Line<'static>>| {
        let mut spans = vec![
            Span::styled(gutter, dim),
            Span::styled(marker.to_string(), marker_style),
        ];
        spans.extend(code_spans(code, lang));
        out.push(Line::from(spans));
    };
    match detail {
        ChangeDetail::Edit { old, new } => {
            for l in old.lines() {
                push("     ".to_string(), del, "- ", l, &mut out);
            }
            for (k, l) in new.lines().enumerate() {
                let n = base_line.saturating_add(k as u32);
                push(format!("{n:>4} "), add, "+ ", l, &mut out);
            }
        }
        ChangeDetail::Write { head } => {
            for (k, l) in head.lines().enumerate() {
                let n = base_line.saturating_add(k as u32);
                push(format!("{n:>4} "), add, "+ ", l, &mut out);
            }
        }
        ChangeDetail::None => {}
    }
    out
}

/// Truncate a styled `Line` to `width` display columns (char-based), preserving
/// span styles; the boundary span is trimmed.
pub fn clip_line_to_width(line: &Line<'static>, width: usize) -> Line<'static> {
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut used = 0;
    for span in &line.spans {
        if used >= width {
            break;
        }
        let remaining = width - used;
        let cnt = span.content.chars().count();
        if cnt <= remaining {
            out.push(span.clone());
            used += cnt;
        } else {
            let truncated: String = span.content.chars().take(remaining).collect();
            out.push(Span::styled(truncated, span.style));
            break;
        }
    }
    Line::from(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::activity::chronology::ChangeDetail;

    fn line_text(line: &Line<'static>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn styled_lines_marker_colors_and_gutter() {
        let detail = ChangeDetail::Edit {
            old: "old".into(),
            new: "let y = 1".into(),
        };
        let lines = change_detail_lines_styled(&detail, 7, lang_for_path(Path::new("a.rs")));
        // removed line: dim gutter (5 spaces), red "- " marker
        assert_eq!(lines[0].spans[0].content.as_ref(), "     ");
        assert!(lines[0].spans[0].style.add_modifier.contains(Modifier::DIM));
        assert_eq!(lines[0].spans[1].content.as_ref(), "- ");
        assert_eq!(lines[0].spans[1].style.fg, Some(Color::Red));
        // added line: gutter "   7 ", green "+ ", code highlighted (let = magenta)
        assert_eq!(lines[1].spans[0].content.as_ref(), "   7 ");
        assert_eq!(lines[1].spans[1].content.as_ref(), "+ ");
        assert_eq!(lines[1].spans[1].style.fg, Some(Color::Green));
        assert!(
            lines[1]
                .spans
                .iter()
                .any(|s| s.content.as_ref() == "let" && s.style.fg == Some(Color::Magenta))
        );
    }

    #[test]
    fn styled_lines_plain_when_no_lang() {
        let detail = ChangeDetail::Write {
            head: "let y = 1".into(),
        };
        let lines = change_detail_lines_styled(&detail, 1, None);
        // code is a single default span (no highlighting): spans = [gutter, marker, code]
        assert_eq!(lines[0].spans[2].content.as_ref(), "let y = 1");
        assert_eq!(lines[0].spans[2].style.fg, None);
    }

    #[test]
    fn clip_line_preserves_styles_and_truncates() {
        let detail = ChangeDetail::Write {
            head: "abcdefgh".into(),
        };
        let line = &change_detail_lines_styled(&detail, 1, None)[0]; // "   1 + abcdefgh"
        let clipped = clip_line_to_width(line, 7);
        assert_eq!(line_text(&clipped), "   1 + ");
        assert_eq!(clip_line_to_width(line, 0).spans.len(), 0);
        let wide = clip_line_to_width(line, 999);
        assert_eq!(line_text(&wide), "   1 + abcdefgh");
    }

    #[test]
    fn lang_for_path_maps_extensions() {
        assert!(lang_for_path(Path::new("a.rs")).is_some());
        assert!(lang_for_path(Path::new("a.py")).is_some());
        assert!(lang_for_path(Path::new("a.c")).is_some());
        assert!(lang_for_path(Path::new("a.js")).is_some());
        assert!(lang_for_path(Path::new("a.sh")).is_some());
        assert!(lang_for_path(Path::new("a.txt")).is_none());
        assert!(lang_for_path(Path::new("noext")).is_none());
    }

    fn texts(spans: &[Span<'static>]) -> Vec<(String, Option<Color>)> {
        spans
            .iter()
            .map(|s| (s.content.to_string(), s.style.fg))
            .collect()
    }

    #[test]
    fn highlight_rust_keyword_string_comment_number() {
        let spans = highlight_code(r#"let x = "hi"; // c"#, &RUST);
        let t = texts(&spans);
        assert!(
            t.iter()
                .any(|(s, c)| s == "let" && *c == Some(Color::Magenta)),
            "{t:?}"
        );
        assert!(
            t.iter()
                .any(|(s, c)| s == "\"hi\"" && *c == Some(Color::Yellow)),
            "{t:?}"
        );
        assert!(
            t.iter()
                .any(|(s, c)| s == "// c" && *c == Some(Color::DarkGray)),
            "{t:?}"
        );

        let nums = highlight_code("x = 42", &RUST);
        assert!(
            texts(&nums)
                .iter()
                .any(|(s, c)| s == "42" && *c == Some(Color::Cyan))
        );
    }

    #[test]
    fn highlight_string_keeps_escaped_quote_in_one_span() {
        let spans = highlight_code(r#""a\"b""#, &RUST);
        let t = texts(&spans);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].0, r#""a\"b""#);
        assert_eq!(t[0].1, Some(Color::Yellow));
    }

    #[test]
    fn non_keyword_identifier_is_default() {
        let spans = highlight_code("foobar", &RUST);
        let t = texts(&spans);
        assert_eq!(t, vec![("foobar".to_string(), None)]);
    }
}
