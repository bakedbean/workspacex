//! Parser for the bar format mini-language, a strict subset of starship's:
//! `$name` / `${name}` inserts a segment, `[text](style)` styles a run,
//! `( … )` renders only when a `$name` inside produced output. `$$` is a
//! literal dollar; `\x` escapes any single character.

use super::style::StyleSpec;

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
fn parse_seq(
    chars: &[char],
    pos: &mut usize,
    until: Option<char>,
) -> Result<Vec<Node>, ParseError> {
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
                    while *pos < chars.len()
                        && (chars[*pos].is_ascii_alphanumeric() || chars[*pos] == '_')
                    {
                        *pos += 1;
                    }
                    chars[start..*pos].iter().collect()
                };
                if name.is_empty() {
                    return Err(err(
                        dollar,
                        "expected a name after `$` (write `$$` for a literal dollar)",
                    ));
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
    fn visit<'a>(nodes: &'a [Node], out: &mut Vec<&'a str>) {
        for node in nodes {
            match node {
                Node::Text(_) => {}
                Node::Var(name) => out.push(name.as_str()),
                Node::Styled(children, _) | Node::Group(children) => visit(children, out),
            }
        }
    }

    let mut out = Vec::new();
    visit(nodes, &mut out);
    out
}

/// Every style spec in `nodes`, depth-first.
pub fn styles(nodes: &[Node]) -> Vec<&StyleSpec> {
    fn visit<'a>(nodes: &'a [Node], out: &mut Vec<&'a StyleSpec>) {
        for node in nodes {
            match node {
                Node::Text(_) | Node::Var(_) => {}
                Node::Styled(children, spec) => {
                    out.push(spec);
                    visit(children, out);
                }
                Node::Group(children) => visit(children, out),
            }
        }
    }

    let mut out = Vec::new();
    visit(nodes, &mut out);
    out
}

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
        assert_eq!(styles(&nodes), vec![&StyleSpec::parse("bold").unwrap()]);
    }

    #[test]
    fn unicode_text_and_escapes_preserve_characters() {
        assert_eq!(
            parse(r"é\界$a終").unwrap(),
            vec![text("é界"), var("a"), text("終")]
        );
    }

    #[test]
    fn unicode_error_offsets_count_characters_not_bytes() {
        assert_eq!(parse("é界[abc").unwrap_err().offset, 2);
        assert_eq!(parse("é[x]終").unwrap_err().offset, 4);
        assert_eq!(parse("界[x](bold").unwrap_err().offset, 4);
        assert_eq!(parse("é${x").unwrap_err().offset, 1);
        assert_eq!(parse(r"é界\").unwrap_err().offset, 2);
    }

    #[test]
    fn mismatched_closers_and_empty_variable_names_are_errors() {
        assert_eq!(parse("[x)").unwrap_err().offset, 2);
        assert_eq!(parse("(x]").unwrap_err().offset, 2);
        assert_eq!(parse("${}").unwrap_err().offset, 0);
        assert_eq!(parse("$").unwrap_err().offset, 0);
    }

    #[test]
    fn nested_styles_walk_in_depth_first_order() {
        let nodes = parse("[a[b](italic)([c](underline))](bold)[d](dimmed)").unwrap();
        let expected =
            ["bold", "italic", "underline", "dimmed"].map(|src| StyleSpec::parse(src).unwrap());
        assert_eq!(styles(&nodes), expected.iter().collect::<Vec<_>>());
    }
}
