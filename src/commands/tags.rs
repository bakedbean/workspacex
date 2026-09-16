//! Prompt tags: XML tag names the attached view wraps a typed body in
//! (`<context>…</context>`), remembered with a use count so the most-used
//! ones surface first. Persisted in the `prompt_tags` setting as one
//! `name=uses` line per tag, the same plain-text shape as `pinned_commands`.

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
        assert_eq!(
            load(&store).unwrap(),
            vec![tag("task", 9), tag("context", 4)]
        );
    }
}
