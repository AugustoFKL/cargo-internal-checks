use std::path::Path;

use derive_more::Display;
use syn::{Item, ItemEnum, ItemMod, Variant, ext::IdentExt, spanned::Spanned};

use super::model::ErrorEnum;
use crate::{
    diagnostic::{Violation as Diagnostic, ViolationKind as DiagnosticKind},
    source::Source,
};

pub(crate) fn check(
    path: &Path,
    source: &Source<'_>,
    items: &[Item],
    violations: &mut Vec<Diagnostic>,
) {
    let mut checker = ErrorVariantChecker::new(path, source);
    checker.check_items(items);
    violations.extend(checker.violations);
}

#[derive(Debug, Clone, PartialEq, Eq, Display)]
pub(crate) enum Violation {
    #[display("error variant `{found}` must appear before `{must_precede}`")]
    Order { found: String, must_precede: String },
    #[display(
        "error variants `{previous}` and `{current}` must be separated by exactly one empty line"
    )]
    Spacing { previous: String, current: String },
    #[display(
        "ordinary comments between error variants `{previous}` and `{current}` are unsupported; use Rustdoc (`///`) on `{current}`"
    )]
    OrdinaryComment { previous: String, current: String },
}

struct ErrorVariantChecker<'a, 'source> {
    path: &'a Path,
    source: &'a Source<'source>,
    module_path: Vec<String>,
    violations: Vec<Diagnostic>,
}

impl<'a, 'source> ErrorVariantChecker<'a, 'source> {
    fn new(path: &'a Path, source: &'a Source<'source>) -> Self {
        Self {
            path,
            source,
            module_path: Vec::new(),
            violations: Vec::new(),
        }
    }

    fn check_items(&mut self, items: &[Item]) {
        for item in items {
            match item {
                Item::Enum(item_enum) => self.check_enum(item_enum),
                Item::Mod(item_mod) => self.check_inline_module(item_mod),
                _ => {}
            }
        }
    }

    fn check_enum(&mut self, item_enum: &ItemEnum) {
        let Some(error_enum) = ErrorEnum::from_ast(item_enum) else {
            return;
        };

        if self.check_ordinary_comments(error_enum) {
            return;
        }

        self.check_variant_order(error_enum);
        self.check_variant_spacing(error_enum);
    }

    fn check_ordinary_comments(&mut self, error_enum: ErrorEnum<'_>) -> bool {
        let mut variants = error_enum.variants().iter();
        let Some(mut previous) = variants.next() else {
            return false;
        };
        let mut found = false;

        for current in variants {
            if self.has_ordinary_comment_between(previous, current) {
                found = true;
                self.report(
                    current.ident.span(),
                    Violation::OrdinaryComment {
                        previous: previous.ident.unraw().to_string(),
                        current: current.ident.unraw().to_string(),
                    },
                );
            }

            previous = current;
        }

        found
    }

    fn check_variant_order(&mut self, error_enum: ErrorEnum<'_>) {
        let mut variants = error_enum.variants().iter();

        let Some(first) = variants.next() else {
            return;
        };

        let mut greatest_seen = first.ident.unraw().to_string();

        for variant in variants {
            let current = variant.ident.unraw().to_string();

            if current >= greatest_seen {
                greatest_seen = current;
                continue;
            }

            self.report(
                variant.ident.span(),
                Violation::Order {
                    found: current,
                    must_precede: greatest_seen.clone(),
                },
            );
        }
    }

    fn check_variant_spacing(&mut self, error_enum: ErrorEnum<'_>) {
        let mut variants = error_enum.variants().iter();
        let Some(mut previous) = variants.next() else {
            return;
        };

        for current in variants {
            let previous_name = previous.ident.unraw().to_string();
            let current_name = current.ident.unraw().to_string();

            if !self.has_one_empty_line(previous.span().end().line, current.span().start().line) {
                self.report(
                    current.ident.span(),
                    Violation::Spacing {
                        previous: previous_name,
                        current: current_name,
                    },
                );
            }

            previous = current;
        }
    }

    fn check_inline_module(&mut self, item_mod: &ItemMod) {
        let Some((_, items)) = &item_mod.content else {
            return;
        };

        self.module_path.push(item_mod.ident.to_string());
        self.check_items(items);
        self.module_path.pop();
    }

    fn has_one_empty_line(&self, previous_end: usize, current_start: usize) -> bool {
        if current_start != previous_end + 2 {
            return false;
        }
        let Some(line) = self.source.text().lines().nth(previous_end) else {
            return false;
        };

        line.trim().is_empty()
    }

    fn has_ordinary_comment_between(&self, previous: &Variant, current: &Variant) -> bool {
        let Some(text) = self
            .source
            .text_between(previous.span().end(), current.span().start())
        else {
            return false;
        };

        text.contains("//") || text.contains("/*")
    }

    fn report(&mut self, span: proc_macro2::Span, kind: Violation) {
        self.violations.push(Diagnostic::at(
            self.path,
            span,
            &self.module_path,
            DiagnosticKind::ErrorVariants(kind),
        ));
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Result;

    use super::*;

    fn violations(source: &str) -> Result<Vec<Diagnostic>> {
        let file = syn::parse_file(source)?;
        let source = Source::new(source);
        let mut violations = Vec::new();
        check(Path::new("lib.rs"), &source, &file.items, &mut violations);
        Ok(violations)
    }

    fn messages(source: &str) -> Result<Vec<String>> {
        let violations = violations(source)?;
        let mut messages = Vec::with_capacity(violations.len());

        for violation in violations {
            messages.push(violation.to_string());
        }

        Ok(messages)
    }

    #[test]
    fn accepts_ordered_variants_of_each_shape() -> Result<()> {
        let source = r#"
#[derive(Debug, thiserror::Error)]
enum Error {
    Alpha,

    Beta(String),

    Gamma { source: String },

    Omega = 4,
}
"#;

        assert!(violations(source)?.is_empty());
        Ok(())
    }

    #[test]
    fn accepts_attributes_and_rustdoc_attached_to_the_following_variant() -> Result<()> {
        let source = r#"
#[derive(Error)]
enum Error {
    #[error("first")]
    First,

    /// The second error.
    #[error("second")]
    Second { source: String },
}
"#;

        assert!(violations(source)?.is_empty());
        Ok(())
    }

    #[test]
    fn accepts_comment_markers_inside_variant_rustdoc() -> Result<()> {
        let source = r#"
#[derive(Error)]
enum Error {
    First,

    /// This should accept // and /*
    Second,
}
"#;

        assert!(violations(source)?.is_empty());
        Ok(())
    }

    #[test]
    fn accepts_empty_and_single_variant_error_enums() -> Result<()> {
        let source = r#"
#[derive(Error)]
enum Empty {}

#[derive(Error)]
enum Singleton {
    Only,
}
"#;

        assert!(violations(source)?.is_empty());
        Ok(())
    }

    #[test]
    fn ignores_enums_that_do_not_derive_error() -> Result<()> {
        let source = r#"
#[derive(Debug)]
enum NotAnError {
    Second,
    First,
}

mod external;
"#;

        assert!(violations(source)?.is_empty());
        Ok(())
    }

    #[test]
    fn reports_every_variant_below_the_greatest_name_seen() -> Result<()> {
        let source = r#"
#[derive(Error)]
enum Error {
    Third,

    First,

    Second,
}
"#;

        assert_eq!(
            messages(source)?,
            [
                "error variant `First` must appear before `Third`",
                "error variant `Second` must appear before `Third`",
            ]
        );
        Ok(())
    }

    #[test]
    fn orders_raw_identifiers_by_their_unraw_name() -> Result<()> {
        let source = r#"
#[derive(Error)]
enum Error {
    r#type,

    r#match,
}
"#;

        assert_eq!(
            messages(source)?,
            ["error variant `match` must appear before `type`"]
        );
        Ok(())
    }

    #[test]
    fn reports_adjacent_variants_without_an_empty_line() -> Result<()> {
        let source = r#"
#[derive(Error)]
enum Error {
    First,
    Second,
}
"#;

        assert_eq!(
            messages(source)?,
            ["error variants `First` and `Second` must be separated by exactly one empty line"]
        );
        Ok(())
    }

    #[test]
    fn reports_adjacent_variants_with_multiple_empty_lines() -> Result<()> {
        let source = r#"
#[derive(Error)]
enum Error {
    First,


    Second,
}
"#;

        assert_eq!(
            messages(source)?,
            ["error variants `First` and `Second` must be separated by exactly one empty line"]
        );
        Ok(())
    }

    #[test]
    fn reports_an_actionable_error_for_a_line_comment_between_variants() -> Result<()> {
        let source = r#"
#[derive(Error)]
enum Error {
    Second(String),

    // RLWE core errors
    First(String),
}
"#;

        assert_eq!(
            messages(source)?,
            [
                "ordinary comments between error variants `Second` and `First` are unsupported; use Rustdoc (`///`) on `First`"
            ]
        );
        Ok(())
    }

    #[test]
    fn reports_all_comments_before_other_violations_in_the_enum() -> Result<()> {
        let source = r#"
#[derive(Error)]
enum Error {
    Third,
    // First section.
    Second,

    /* Second section. */
    First,
}
"#;

        assert_eq!(
            messages(source)?,
            [
                "ordinary comments between error variants `Third` and `Second` are unsupported; use Rustdoc (`///`) on `Second`",
                "ordinary comments between error variants `Second` and `First` are unsupported; use Rustdoc (`///`) on `First`",
            ]
        );
        Ok(())
    }

    #[test]
    fn reports_an_actionable_error_for_a_trailing_comment_between_variants() -> Result<()> {
        let source = r#"
#[derive(Error)]
enum Error {
    First, // section comment

    Second,
}
"#;

        assert_eq!(
            messages(source)?,
            [
                "ordinary comments between error variants `First` and `Second` are unsupported; use Rustdoc (`///`) on `Second`"
            ]
        );
        Ok(())
    }

    #[test]
    fn reports_an_actionable_error_for_a_block_comment_between_variants() -> Result<()> {
        let source = r#"
#[derive(Error)]
enum Error {
    First,

    /* section comment */
    Second,
}
"#;

        assert_eq!(
            messages(source)?,
            [
                "ordinary comments between error variants `First` and `Second` are unsupported; use Rustdoc (`///`) on `Second`"
            ]
        );
        Ok(())
    }

    #[test]
    fn reports_order_and_spacing_independently() -> Result<()> {
        let source = r#"
#[derive(Error)]
enum Error {
    Second,
    First,
}
"#;

        assert_eq!(
            messages(source)?,
            [
                "error variant `First` must appear before `Second`",
                "error variants `Second` and `First` must be separated by exactly one empty line",
            ]
        );
        Ok(())
    }

    #[test]
    fn checks_inline_modules_recursively() -> Result<()> {
        let source = r#"
mod outer {
    mod inner {
        #[derive(Error)]
        enum Error {
            Second,

            First,
        }
    }
}
"#;

        let violations = violations(source)?;
        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].to_string(),
            "error variant `First` must appear before `Second`"
        );
        assert_eq!(violations[0].module_path(), ["outer", "inner"]);

        Ok(())
    }
}
