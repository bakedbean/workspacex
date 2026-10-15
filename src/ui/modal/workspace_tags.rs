//! Rows for the `#` workspace-tags editor.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagItem {
    pub name: String,
    pub checked: bool,
    pub new: bool,
}

/// `known` ∪ `tags`, sorted, narrowed by `input`; a valid unseen `input`
/// leads as a `(new)` row.
pub fn tag_items(known: &[String], tags: &[String], input: &str) -> Vec<TagItem> {
    let needle = input.trim().to_lowercase();
    let mut names: Vec<&String> = known.iter().chain(tags).collect();
    names.sort();
    names.dedup();
    let mut out: Vec<TagItem> = Vec::new();
    if let Some(n) = crate::data::tags::normalize_tag(input)
        && !names.iter().any(|k| **k == n)
    {
        out.push(TagItem {
            name: n,
            checked: false,
            new: true,
        });
    }
    out.extend(
        names
            .into_iter()
            .filter(|k| k.contains(&needle))
            .map(|k| TagItem {
                name: k.clone(),
                checked: tags.contains(k),
                new: false,
            }),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn lists_known_and_checks_current() {
        let items = tag_items(&s(&["parked", "ui"]), &s(&["ui"]), "");
        assert_eq!(
            items
                .iter()
                .map(|i| (i.name.as_str(), i.checked))
                .collect::<Vec<_>>(),
            [("parked", false), ("ui", true)]
        );
    }

    #[test]
    fn filters_by_substring_and_offers_new_first() {
        let items = tag_items(&s(&["parked", "spike"]), &[], "Sp");
        assert_eq!(
            items[0],
            TagItem {
                name: "sp".into(),
                checked: false,
                new: true
            }
        );
        assert_eq!(items[1].name, "spike");
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn no_new_row_for_existing_or_invalid_input() {
        assert!(
            tag_items(&s(&["spike"]), &[], "spike")
                .iter()
                .all(|i| !i.new)
        );
        assert!(tag_items(&[], &[], "a b").is_empty());
    }
}
