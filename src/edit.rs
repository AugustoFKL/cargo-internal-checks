pub(crate) struct Edit {
    start: usize,
    end: usize,
    replacement: String,
}

impl Edit {
    pub(crate) fn new(start: usize, end: usize, replacement: String) -> Self {
        Self {
            start,
            end,
            replacement,
        }
    }

    pub(crate) fn apply_all(mut source: String, mut edits: Vec<Self>) -> String {
        edits.sort_unstable_by_key(|edit| edit.start);

        // Applying edits from right to left preserves every earlier byte offset.
        for edit in edits.into_iter().rev() {
            source.replace_range(edit.start..edit.end, &edit.replacement);
        }

        source
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_edits_without_invalidating_earlier_offsets() {
        let edits = vec![
            Edit {
                start: 0,
                end: 1,
                replacement: "first".to_owned(),
            },
            Edit {
                start: 2,
                end: 3,
                replacement: "third".to_owned(),
            },
        ];

        assert_eq!(Edit::apply_all("a b".to_owned(), edits), "first third");
    }
}
