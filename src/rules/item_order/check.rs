use std::{cmp::Ordering, path::Path};

use derive_more::Display;
use proc_macro2::Span;
use syn::{Item, ItemMod};

use super::{
    ItemClass, ItemGroup,
    model::{ClassifiedItem, ImportKey, ModuleScope},
};
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
    let mut checker = ItemOrderChecker::new(path, source);
    checker.check_items(items);
    violations.extend(checker.violations);
}

#[derive(Debug, Clone, PartialEq, Eq, Display)]
pub(crate) enum Violation {
    #[display(
        "items from `{previous}` and `{current}` groups with different class or origin must be separated by a blank line"
    )]
    MissingBlankLine {
        previous: ItemGroup,
        current: ItemGroup,
    },
    #[display(
        "imports with `{group}` origin and the same visibility must not be separated by a blank line"
    )]
    UnexpectedBlankLine { group: ItemGroup },
    #[display(
        "ordinary comments between `{previous}` and `{current}` items are unsupported; use Rustdoc (`///`) on the following item or move the comment outside the ordered run"
    )]
    OrdinaryComment {
        previous: ItemClass,
        current: ItemClass,
    },
    #[display("test module `tests` must be private")]
    TestModuleVisibility,
    #[display("`{found}` must appear before `{must_precede}`")]
    ItemOrder {
        found: ItemClass,
        must_precede: ItemClass,
    },
    #[display("import `{found}` must appear before `{must_precede}`")]
    ImportOrder { found: String, must_precede: String },
}

struct ItemOrderChecker<'a, 'source> {
    path: &'a Path,
    source: &'a Source<'source>,
    module_path: Vec<String>,
    violations: Vec<Diagnostic>,
}

impl<'a, 'source> ItemOrderChecker<'a, 'source> {
    fn new(path: &'a Path, source: &'a Source<'source>) -> Self {
        Self {
            path,
            source,
            module_path: Vec::new(),
            violations: Vec::new(),
        }
    }

    fn check_items(&mut self, items: &[Item]) {
        let scope = ModuleScope::from_items(items);
        let mut run = Vec::new();

        for item in items {
            self.check_test_module_visibility(item);

            if let Some(classified) = ClassifiedItem::from_ast(item, &scope) {
                run.push(classified);
            } else {
                self.check_run(&run);
                run.clear();
            }
        }
        self.check_run(&run);

        for item in items {
            self.check_inline_module(item);
        }
    }

    fn check_run(&mut self, items: &[ClassifiedItem]) {
        if self.check_ordinary_comments(items) {
            return;
        }

        let mut run = OrderedRun::default();
        for item in items {
            self.check_classified(item, &mut run);
        }
    }

    fn check_classified(&mut self, classified: &ClassifiedItem, run: &mut OrderedRun) {
        self.check_group_separation(classified, run);
        self.check_order(classified, run);
        run.remember_item(classified);
    }

    fn check_test_module_visibility(&mut self, item: &Item) {
        let Item::Mod(item) = item else {
            return;
        };

        if ClassifiedItem::is_conventional_test_module(item)
            && !matches!(&item.vis, syn::Visibility::Inherited)
        {
            self.report(item.mod_token.span, Violation::TestModuleVisibility);
        }
    }

    fn check_ordinary_comments(&mut self, items: &[ClassifiedItem]) -> bool {
        let mut found = false;

        for pair in items.windows(2) {
            if !self.has_ordinary_comment_between(&pair[0], &pair[1]) {
                continue;
            }

            found = true;
            self.report(
                pair[1].span,
                Violation::OrdinaryComment {
                    previous: pair[0].class,
                    current: pair[1].class,
                },
            );
        }

        found
    }

    fn has_ordinary_comment_between(
        &self,
        previous: &ClassifiedItem,
        current: &ClassifiedItem,
    ) -> bool {
        let Some(text) = self
            .source
            .text_between(previous.full_span.end(), current.full_span.start())
        else {
            return false;
        };

        text.contains("//") || text.contains("/*")
    }

    fn check_group_separation(&mut self, current: &ClassifiedItem, run: &OrderedRun) {
        let current_group = current.group();
        let Some(previous) = run.previous_item else {
            return;
        };

        let group_changed = current.class != previous.class || current_group != previous.group;
        let has_blank_line = self.has_blank_line(previous.end_line, current.span.start().line);
        if group_changed && !has_blank_line {
            self.report(
                current.span,
                Violation::MissingBlankLine {
                    previous: previous.group,
                    current: current_group,
                },
            );
        } else if !group_changed && current.import_key.is_some() && has_blank_line {
            self.report(
                current.span,
                Violation::UnexpectedBlankLine {
                    group: current_group,
                },
            );
        }
    }

    fn check_order(&mut self, current: &ClassifiedItem, run: &mut OrderedRun) {
        let rank = current.class.rank();
        let Some(highest) = run.highest.as_ref() else {
            run.highest = Some(RankedItem::new(current, rank));
            return;
        };

        if rank < highest.rank {
            self.report(
                current.span,
                Violation::ItemOrder {
                    found: current.class,
                    must_precede: highest.class,
                },
            );
            return;
        } else if rank > highest.rank {
            run.highest = Some(RankedItem::new(current, rank));
            return;
        }

        let (Some(current_key), Some(highest_key)) = (&current.import_key, &highest.import_key)
        else {
            return;
        };

        match current_key.group.cmp(&highest_key.group) {
            Ordering::Less => self.report(
                current.span,
                Violation::ImportOrder {
                    found: current_key.path.clone(),
                    must_precede: highest_key.path.clone(),
                },
            ),
            Ordering::Equal => {}
            Ordering::Greater => run.highest = Some(RankedItem::new(current, rank)),
        }
    }

    fn check_inline_module(&mut self, item: &Item) {
        let Item::Mod(ItemMod {
            content: Some((_, nested_items)),
            ident,
            ..
        }) = item
        else {
            return;
        };

        self.module_path.push(ident.to_string());
        self.check_items(nested_items);
        self.module_path.pop();
    }

    fn has_blank_line(&self, previous_end_line: usize, current_start_line: usize) -> bool {
        self.source
            .text()
            .lines()
            .skip(previous_end_line)
            .take(current_start_line.saturating_sub(previous_end_line + 1))
            .any(|line| line.trim().is_empty())
    }

    fn report(&mut self, span: Span, kind: Violation) {
        self.violations.push(Diagnostic::at(
            self.path,
            span,
            &self.module_path,
            DiagnosticKind::ItemOrder(kind),
        ));
    }
}

#[derive(Default)]
struct OrderedRun {
    highest: Option<RankedItem>,
    previous_item: Option<PreviousItem>,
}

impl OrderedRun {
    fn remember_item(&mut self, item: &ClassifiedItem) {
        self.previous_item = Some(PreviousItem {
            class: item.class,
            group: item.group(),
            end_line: item.end_line,
        });
    }
}

struct RankedItem {
    rank: usize,
    class: ItemClass,
    import_key: Option<ImportKey>,
}

impl RankedItem {
    fn new(item: &ClassifiedItem, rank: usize) -> Self {
        Self {
            rank,
            class: item.class,
            import_key: item.import_key.clone(),
        }
    }
}

#[derive(Clone, Copy)]
struct PreviousItem {
    class: ItemClass,
    group: ItemGroup,
    end_line: usize,
}

#[cfg(test)]
mod tests {
    use anyhow::Result;

    use super::*;
    use crate::rules::item_order::model::{ItemKind, Visibility};

    fn check_source(path: &Path, source: &str) -> Result<Vec<Diagnostic>> {
        let file = syn::parse_file(source)?;
        let source = Source::new(source);
        let mut violations = Vec::new();
        check(path, &source, &file.items, &mut violations);
        Ok(violations)
    }

    #[test]
    fn accepts_ordered_run() -> Result<()> {
        let violations = check_source(
            Path::new("lib.rs"),
            r#"
            use crate::a::A;

            pub(crate) use crate::b::B;

            pub use crate::c::C;

            mod d;
            mod d2;

            pub(crate) mod e;

            pub mod f;
            "#,
        )?;

        assert!(violations.is_empty());

        Ok(())
    }

    #[test]
    fn reports_out_of_order_item() -> Result<()> {
        let violations = check_source(
            Path::new("lib.rs"),
            r#"
            pub use crate::a::A;

            pub(crate) use crate::b::B;
            "#,
        )?;

        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].kind(),
            &DiagnosticKind::ItemOrder(Violation::ItemOrder {
                found: ItemClass::new(ItemKind::Use, Visibility::Crate),
                must_precede: ItemClass::new(ItemKind::Use, Visibility::Public),
            })
        );
        Ok(())
    }

    #[test]
    fn reports_local_import_before_external_import() -> Result<()> {
        let violations = check_source(
            Path::new("lib.rs"),
            r#"
            use anyhow::{Context, Result};
            use proc_macro2::{LineColumn, Span};
            use syn::{Item, ItemMod};

            use crate::diagnostic::{Violation, ViolationKind};

            use tracing::info;
            "#,
        )?;

        assert_eq!(violations.len(), 1);
        Ok(())
    }

    #[test]
    fn accepts_grouped_imports_without_alphabetizing_within_groups() -> Result<()> {
        let violations = check_source(
            Path::new("lib.rs"),
            r#"
            use std::path::Path;

            use tracing::info;
            use anyhow::Result as AnyhowResult;

            use crate::diagnostic::Violation;
            use self::local::Local;
            use super::parent::Parent;
            "#,
        )?;

        assert!(violations.is_empty());
        Ok(())
    }

    #[test]
    fn accepts_adjacent_imports_in_the_same_group() -> Result<()> {
        let violations = check_source(
            Path::new("lib.rs"),
            r#"
            use crate::encoding::Decode;
            use crate::schemes::NgfheScheme;
            "#,
        )?;

        assert!(violations.is_empty());
        Ok(())
    }

    #[test]
    fn rejects_blank_line_between_imports_in_the_same_group() -> Result<()> {
        let violations = check_source(
            Path::new("lib.rs"),
            r#"
            use crate::encoding::Decode;

            use crate::schemes::NgfheScheme;
            "#,
        )?;

        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].kind(),
            &DiagnosticKind::ItemOrder(Violation::UnexpectedBlankLine {
                group: ItemGroup::Local,
            })
        );
        Ok(())
    }

    #[test]
    fn requires_blank_line_between_external_and_local_imports() -> Result<()> {
        let violations = check_source(
            Path::new("lib.rs"),
            r#"
            use anyhow::Context;
            use super::*;
            use crate::diagnostic::{Violation, ViolationKind};
            "#,
        )?;

        assert_eq!(violations.len(), 1);
        assert!(matches!(
            violations[0].kind(),
            DiagnosticKind::ItemOrder(Violation::MissingBlankLine { .. })
        ));
        Ok(())
    }

    #[test]
    fn unrelated_item_ends_a_run() -> Result<()> {
        let violations = check_source(
            Path::new("lib.rs"),
            r#"
            pub use crate::a::A;

            const VALUE: usize = 3;

            use crate::b::B;
            "#,
        )?;

        assert!(violations.is_empty());
        Ok(())
    }

    #[test]
    fn reports_comment_instead_of_blocked_run_violations() -> Result<()> {
        let violations = check_source(
            Path::new("lib.rs"),
            r#"
            use crate::a::A;

            use crate::b::B;
            // Module declarations.
            mod c;
            "#,
        )?;

        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].kind(),
            &DiagnosticKind::ItemOrder(Violation::OrdinaryComment {
                previous: ItemClass::new(ItemKind::Use, Visibility::Private),
                current: ItemClass::new(ItemKind::Mod, Visibility::Private),
            })
        );
        Ok(())
    }

    #[test]
    fn accepts_rustdoc_on_an_ordered_item() -> Result<()> {
        let violations = check_source(
            Path::new("lib.rs"),
            r#"
            use crate::a::A;

            /// The C module.
            mod c;
            "#,
        )?;

        assert!(violations.is_empty());
        Ok(())
    }

    #[test]
    fn checks_inline_modules_recursively() -> Result<()> {
        let violations = check_source(
            Path::new("lib.rs"),
            r#"
            mod outer {
                mod inner {
                    pub mod public;

                    use crate::a::A;
                }
            }
            "#,
        )?;

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].module_path(), ["outer", "inner"]);
        Ok(())
    }

    #[test]
    fn every_supported_item_class_participates_in_the_run() -> Result<()> {
        let violations = check_source(
            Path::new("lib.rs"),
            r#"
            pub use crate::a::A;

            mod boundary;

            use crate::b::B;
            "#,
        )?;

        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].kind(),
            &DiagnosticKind::ItemOrder(Violation::ItemOrder {
                found: ItemClass::new(ItemKind::Use, Visibility::Private),
                must_precede: ItemClass::new(ItemKind::Mod, Visibility::Private),
            })
        );
        Ok(())
    }

    #[test]
    fn accepts_conventional_test_module_after_ordinary_modules() -> Result<()> {
        let violations = check_source(
            Path::new("lib.rs"),
            r#"
            #[cfg(test)]
            mod test_support {}

            pub mod code_module {}

            #[cfg(test)]
            mod tests {}
            "#,
        )?;

        assert!(violations.is_empty());
        Ok(())
    }

    #[test]
    fn requires_conventional_test_module_after_ordinary_modules() -> Result<()> {
        let violations = check_source(
            Path::new("lib.rs"),
            r#"
            #[cfg(test)]
            mod tests {}

            pub mod code_module {}
            "#,
        )?;

        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].kind(),
            &DiagnosticKind::ItemOrder(Violation::ItemOrder {
                found: ItemClass::new(ItemKind::Mod, Visibility::Public),
                must_precede: ItemClass::new(ItemKind::TestModule, Visibility::Private),
            })
        );
        Ok(())
    }

    #[test]
    fn requires_conventional_test_module_to_be_private() -> Result<()> {
        let violations = check_source(
            Path::new("lib.rs"),
            r#"
            #[cfg(test)]
            pub mod tests {}
            "#,
        )?;

        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].kind(),
            &DiagnosticKind::ItemOrder(Violation::TestModuleVisibility)
        );
        Ok(())
    }

    #[test]
    fn unsupported_restricted_visibility_is_a_run_boundary() -> Result<()> {
        let violations = check_source(
            Path::new("lib.rs"),
            r#"
            pub use crate::a::A;
            pub(super) use crate::b::B;
            use crate::c::C;
            "#,
        )?;

        assert!(violations.is_empty());
        Ok(())
    }

    #[test]
    fn treats_bare_imports_of_declared_modules_as_local() -> Result<()> {
        let violations = check_source(
            Path::new("lib.rs"),
            r#"
            pub use core::*;

            mod core;
            "#,
        )?;

        assert!(violations.is_empty());
        Ok(())
    }

    #[test]
    fn treats_core_as_standard_library_without_a_local_module() -> Result<()> {
        let violations = check_source(
            Path::new("lib.rs"),
            r#"
            use tracing::info;

            use core::fmt;
            "#,
        )?;

        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].kind(),
            &DiagnosticKind::ItemOrder(Violation::ImportOrder {
                found: "core::fmt".to_owned(),
                must_precede: "tracing::info".to_owned(),
            })
        );
        Ok(())
    }

    #[test]
    fn attributed_imports_participate_in_group_ordering() -> Result<()> {
        let violations = check_source(
            Path::new("lib.rs"),
            r#"
            use crate::local::Local;

            #[cfg(feature = "optional")]
            use anyhow::Result;
            "#,
        )?;

        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].kind(),
            &DiagnosticKind::ItemOrder(Violation::ImportOrder {
                found: "anyhow::Result".to_owned(),
                must_precede: "crate::local::Local".to_owned(),
            })
        );
        Ok(())
    }
}
