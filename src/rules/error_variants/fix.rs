use syn::{Item, ItemEnum, ItemMod, ext::IdentExt, spanned::Spanned};

use super::model::ErrorEnum;
use crate::{edit::Edit, source::Source};

pub(crate) fn edits(source: &Source<'_>, items: &[Item]) -> Vec<Edit> {
    let fixer = ErrorVariantFixer::new(source);
    let mut edits = Vec::new();
    fixer.collect_edits_into(items, &mut edits);
    edits
}

struct ErrorVariantFixer<'a, 'source> {
    source: &'a Source<'source>,
}

impl<'a, 'source> ErrorVariantFixer<'a, 'source> {
    fn new(source: &'a Source<'source>) -> Self {
        Self { source }
    }

    fn collect_edits_into(&self, items: &[Item], edits: &mut Vec<Edit>) {
        for item in items {
            match item {
                Item::Enum(item_enum) => {
                    if let Some(edit) = self.edit_for_enum(item_enum) {
                        edits.push(edit);
                    }
                }
                Item::Mod(ItemMod {
                    content: Some((_, nested_items)),
                    ..
                }) => self.collect_edits_into(nested_items, edits),
                _ => {}
            }
        }
    }

    fn edit_for_enum(&self, item_enum: &ItemEnum) -> Option<Edit> {
        let error_enum = ErrorEnum::from_ast(item_enum)?;
        let run = VariantRun::from_error_enum(error_enum, self.source)?;
        if run.len() < 2 || run.has_ambiguous_text(self.source.text()) {
            return None;
        }

        let mut sorted = run.variants.clone();
        sorted.sort_by(|left, right| left.name.cmp(&right.name));

        let indentation = self.source.indentation_at(run.start());
        let replacement = self.render_variants(&sorted, indentation, run.has_trailing_comma);
        let start = run.start();
        let end = run.end();

        if replacement == self.source.text()[start..end] {
            return None;
        }

        Some(Edit::new(start, end, replacement))
    }

    fn render_variants(
        &self,
        variants: &[FixableVariant],
        indentation: &str,
        has_trailing_comma: bool,
    ) -> String {
        let mut replacement = String::new();

        for (index, variant) in variants.iter().enumerate() {
            if index > 0 {
                replacement.push_str(self.source.newline());
                replacement.push_str(self.source.newline());
                replacement.push_str(indentation);
            }

            replacement.push_str(&self.source.text()[variant.start..variant.body_end]);

            // Commas belong to positions, not movable variant slices. Rendering
            // them here keeps a moved, originally-last variant syntactically valid.
            if index + 1 < variants.len() || has_trailing_comma {
                replacement.push(',');
            }
        }

        replacement
    }
}

struct VariantRun {
    variants: Vec<FixableVariant>,
    has_trailing_comma: bool,
    content_start: usize,
    content_end: usize,
}

impl VariantRun {
    fn from_error_enum(error_enum: ErrorEnum<'_>, source: &Source<'_>) -> Option<Self> {
        let item_enum = error_enum.ast();
        let mut variants = Vec::with_capacity(error_enum.variants().len());

        for pair in error_enum.variants().pairs() {
            let variant = pair.value();
            let span = variant.span();
            let body_end = source.offset(span.end())?;
            let full_end = match pair.punct() {
                Some(comma) => source.offset(comma.span().end())?,
                None => body_end,
            };

            variants.push(FixableVariant {
                start: source.offset(span.start())?,
                body_end,
                full_end,
                name: variant.ident.unraw().to_string(),
            });
        }

        Some(Self {
            variants,
            has_trailing_comma: error_enum.variants().trailing_punct(),
            content_start: source.offset(item_enum.brace_token.span.open().end())?,
            content_end: source.offset(item_enum.brace_token.span.close().start())?,
        })
    }

    fn len(&self) -> usize {
        self.variants.len()
    }

    fn start(&self) -> usize {
        self.variants[0].start
    }

    fn end(&self) -> usize {
        self.variants[self.variants.len() - 1].full_end
    }

    fn has_ambiguous_text(&self, source: &str) -> bool {
        self.has_boundary_text(source)
            || self.has_nonstandard_punctuation(source)
            || self.has_inter_variant_text(source)
    }

    fn has_boundary_text(&self, source: &str) -> bool {
        !source[self.content_start..self.start()].trim().is_empty()
            || !source[self.end()..self.content_end].trim().is_empty()
    }

    fn has_nonstandard_punctuation(&self, source: &str) -> bool {
        for variant in &self.variants {
            let punctuation = source[variant.body_end..variant.full_end].trim();
            if !punctuation.is_empty() && punctuation != "," {
                return true;
            }
        }

        false
    }

    fn has_inter_variant_text(&self, source: &str) -> bool {
        for pair in self.variants.windows(2) {
            if !source[pair[0].full_end..pair[1].start].trim().is_empty() {
                return true;
            }
        }

        false
    }
}

#[derive(Clone)]
struct FixableVariant {
    start: usize,
    body_end: usize,
    full_end: usize,
    name: String,
}

#[cfg(test)]
mod tests {
    use anyhow::Result;

    use crate::fix::fix_source;

    fn fixed(source: &str) -> Result<String> {
        fix_source(source)
    }

    #[test]
    fn orders_variants_and_preserves_attributes_and_bodies() -> Result<()> {
        let source = r#"#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("third")]
    Third { source: String },
    /// The first error.
    #[error("first")]
    First,


    #[error("second")]
    Second(String),
}
"#;

        let expected = r#"#[derive(Debug, thiserror::Error)]
enum Error {
    /// The first error.
    #[error("first")]
    First,

    #[error("second")]
    Second(String),

    #[error("third")]
    Third { source: String },
}
"#;

        assert_eq!(fixed(source)?, expected);
        Ok(())
    }

    #[test]
    fn normalizes_spacing_without_reordering() -> Result<()> {
        let source = r#"#[derive(Error)]
enum Error {
    First,
    Second,


    Third,
}
"#;

        let expected = r#"#[derive(Error)]
enum Error {
    First,

    Second,

    Third,
}
"#;

        assert_eq!(fixed(source)?, expected);
        Ok(())
    }

    #[test]
    fn orders_variants_by_their_unraw_names() -> Result<()> {
        let source = "#[derive(Error)]\nenum Error {\n    r#type,\n    r#match,\n}\n";
        let expected = "#[derive(Error)]\nenum Error {\n    r#match,\n\n    r#type,\n}\n";

        assert_eq!(fixed(source)?, expected);
        Ok(())
    }

    #[test]
    fn preserves_the_trailing_comma_policy() -> Result<()> {
        let source = "#[derive(Error)]\nenum Error {\n    Second,\n    First\n}\n";
        let expected = "#[derive(Error)]\nenum Error {\n    First,\n\n    Second\n}\n";

        assert_eq!(fixed(source)?, expected);
        Ok(())
    }

    #[test]
    fn preserves_crlf() -> Result<()> {
        let source = "#[derive(Error)]\r\nenum Error {\r\n    Second,\r\n    First,\r\n}\r\n";
        let expected = "#[derive(Error)]\r\nenum Error {\r\n    First,\r\n\r\n    Second,\r\n}\r\n";

        assert_eq!(fixed(source)?, expected);
        Ok(())
    }

    #[test]
    fn fixes_variants_inside_inline_modules() -> Result<()> {
        let source = r#"mod outer {
    mod inner {
        #[derive(Error)]
        enum Error {
            Second,
            First,
        }
    }
}
"#;

        let expected = r#"mod outer {
    mod inner {
        #[derive(Error)]
        enum Error {
            First,

            Second,
        }
    }
}
"#;

        assert_eq!(fixed(source)?, expected);
        Ok(())
    }

    #[test]
    fn ignores_enums_that_do_not_derive_error() -> Result<()> {
        let source = "#[derive(Debug)]\nenum Example {\n    Second,\n    First,\n}\n";

        assert_eq!(fixed(source)?, source);
        Ok(())
    }

    #[test]
    fn leaves_inter_variant_comments_unchanged() -> Result<()> {
        let source = r#"#[derive(Error)]
enum Error {
    Second,
    // The first error needs additional context.
    First,
}
"#;

        assert_eq!(fixed(source)?, source);
        Ok(())
    }

    #[test]
    fn leaves_boundary_comments_unchanged() -> Result<()> {
        let before_first = r#"#[derive(Error)]
enum Error {
    // This comment belongs to Second.
    Second,
    First,
}
"#;
        let after_last = r#"#[derive(Error)]
enum Error {
    Second,
    First, // This comment belongs to First.
}
"#;

        assert_eq!(fixed(before_first)?, before_first);
        assert_eq!(fixed(after_last)?, after_last);
        Ok(())
    }
}
