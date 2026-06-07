//! Basic, dependency-free syntax highlighting for the chronology detail modal.
//! A single generic tokenizer driven by a per-language `LangSpec`. Per-line,
//! no cross-line state — "basic" fidelity for a glanceable diff.

use ratatui::style::{Color, Style};
use ratatui::text::Span;
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

#[cfg(test)]
mod tests {
    use super::*;

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
