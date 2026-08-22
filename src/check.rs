use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use derive_more::Display;
use itertools::Itertools;
use proc_macro2::{LineColumn, Span};
use syn::{Item, ItemMod, UseTree};

use crate::config::{Config, ItemClass, ItemKind, Visibility};

#[derive(Debug, Clone, PartialEq, Eq, Display)]
#[display("{kind}")]
pub(crate) struct Violation {
    path: PathBuf,
    line: usize,
    column: usize,
    module_path: Vec<String>,
    kind: ViolationKind,
}

impl Violation {
    pub(crate) fn path(&self) -> &PathBuf {
        &self.path
    }

    pub(crate) fn line(&self) -> usize {
        self.line
    }

    pub(crate) fn column(&self) -> usize {
        self.column
    }

    pub(crate) fn module_path(&self) -> &[String] {
        &self.module_path
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Display)]
enum ViolationKind {
    #[display("`{found}` must appear before `{must_precede}`")]
    ItemOrder {
        found: ItemClass,
        must_precede: ItemClass,
    },
    #[display("import `{found}` must appear before `{must_precede}`")]
    ImportOrder { found: String, must_precede: String },
    #[display(
        "imports from `{previous}` and `{current}` groups with different origin or visibility must be separated by a blank line"
    )]
    MissingBlankLine {
        previous: ImportGroup,
        current: ImportGroup,
    },
}

pub(crate) fn check_file(path: &Path, config: &Config) -> Result<Vec<Violation>> {
    let source = fs::read_to_string(path)
        .with_context(|| format!("failed to read Rust source `{}`", path.display()))?;
    check_source(path, &source, config)
}

fn check_source(path: &Path, source: &str, config: &Config) -> Result<Vec<Violation>> {
    let file = syn::parse_file(source)
        .with_context(|| format!("failed to parse Rust source `{}`", path.display()))?;
    Ok(Analyzer::new(path, source, config).analyze(&file.items))
}

struct Analyzer<'a> {
    path: &'a Path,
    source: &'a str,
    config: &'a Config,
    module_path: Vec<String>,
    violations: Vec<Violation>,
}

impl<'a> Analyzer<'a> {
    fn new(path: &'a Path, source: &'a str, config: &'a Config) -> Self {
        Self {
            path,
            source,
            config,
            module_path: Vec::new(),
            violations: Vec::new(),
        }
    }

    fn analyze(mut self, items: &[Item]) -> Vec<Violation> {
        self.check_items(items);
        self.violations
    }

    fn check_items(&mut self, items: &[Item]) {
        let mut run = OrderedRun::default();

        for item in items {
            match ClassifiedItem::from_ast(item)
                .and_then(|item| self.config.rank(item.class).map(|rank| (item, rank)))
            {
                Some((classified, rank)) => self.check_classified(classified, rank, &mut run),
                None => run.clear(),
            }

            self.check_inline_module(item);
        }
    }

    fn check_classified(&mut self, classified: ClassifiedItem, rank: usize, run: &mut OrderedRun) {
        self.check_group_separation(&classified, run);
        self.check_order(&classified, rank, run);
        run.remember_import(&classified);
    }

    fn check_group_separation(&mut self, current: &ClassifiedItem, run: &OrderedRun) {
        let Some(current_key) = current.import_key.as_ref() else {
            return;
        };
        let current_group = current_key.origin.group();
        let Some(previous) = run.previous_import else {
            return;
        };

        let group_changed = current.class != previous.class || current_group != previous.group;
        if group_changed && !self.has_blank_line(previous.end_line, current.span.start().line) {
            self.report(
                current.span,
                ViolationKind::MissingBlankLine {
                    previous: previous.group,
                    current: current_group,
                },
            );
        }
    }

    fn check_order(&mut self, current: &ClassifiedItem, rank: usize, run: &mut OrderedRun) {
        let Some(highest) = run.highest.as_ref() else {
            run.highest = Some(RankedItem::new(current, rank));
            return;
        };

        let import_order = current
            .import_key
            .as_ref()
            .zip(highest.import_key.as_ref())
            .filter(|_| rank == highest.rank);

        if rank < highest.rank {
            self.report(
                current.span,
                ViolationKind::ItemOrder {
                    found: current.class,
                    must_precede: highest.class,
                },
            );
        } else if let Some((current_key, highest_key)) =
            import_order.filter(|(current, highest)| current < highest)
        {
            self.report(
                current.span,
                ViolationKind::ImportOrder {
                    found: current_key.path.clone(),
                    must_precede: highest_key.path.clone(),
                },
            );
        } else if rank > highest.rank || current.import_key.as_ref() > highest.import_key.as_ref() {
            run.highest = Some(RankedItem::new(current, rank));
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
            .lines()
            .skip(previous_end_line)
            .take(current_start_line.saturating_sub(previous_end_line + 1))
            .any(|line| line.trim().is_empty())
    }

    fn report(&mut self, span: Span, kind: ViolationKind) {
        let LineColumn { line, column } = span.start();
        self.violations.push(Violation {
            path: self.path.to_path_buf(),
            line,
            column: column + 1,
            module_path: self.module_path.clone(),
            kind,
        });
    }
}

#[derive(Default)]
struct OrderedRun {
    highest: Option<RankedItem>,
    previous_import: Option<PreviousImport>,
}

impl OrderedRun {
    fn clear(&mut self) {
        self.highest = None;
        self.previous_import = None;
    }

    fn remember_import(&mut self, item: &ClassifiedItem) {
        self.previous_import = item.import_key.as_ref().map(|key| PreviousImport {
            class: item.class,
            group: key.origin.group(),
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
struct PreviousImport {
    class: ItemClass,
    group: ImportGroup,
    end_line: usize,
}

#[derive(Debug)]
struct ClassifiedItem {
    class: ItemClass,
    span: Span,
    end_line: usize,
    import_key: Option<ImportKey>,
}

impl ClassifiedItem {
    fn from_ast(item: &Item) -> Option<Self> {
        match item {
            Item::Use(item) => Some(Self {
                class: ItemClass::new(ItemKind::Use, Self::visibility(&item.vis)?),
                span: item.use_token.span,
                end_line: item.semi_token.span.end().line,
                import_key: Some(ImportKey::from_tree(&item.tree)),
            }),
            Item::Mod(item) => Some(Self {
                class: ItemClass::new(ItemKind::Mod, Self::visibility(&item.vis)?),
                span: item.mod_token.span,
                end_line: item.semi.as_ref().map_or_else(
                    || item.mod_token.span.end().line,
                    |semi| semi.span.end().line,
                ),
                import_key: None,
            }),
            _ => None,
        }
    }

    fn visibility(visibility: &syn::Visibility) -> Option<Visibility> {
        match visibility {
            syn::Visibility::Inherited => Some(Visibility::Private),
            syn::Visibility::Public(_) => Some(Visibility::Public),
            syn::Visibility::Restricted(visibility)
                if visibility.in_token.is_none() && visibility.path.is_ident("crate") =>
            {
                Some(Visibility::Crate)
            }
            syn::Visibility::Restricted(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ImportKey {
    origin: ImportOrigin,
    path: String,
}

impl ImportKey {
    fn from_tree(tree: &UseTree) -> Self {
        let path = Self::tree_path(tree);
        let origin = match path.split("::").next().unwrap_or_default() {
            "std" | "core" | "alloc" => ImportOrigin::StandardLibrary,
            "self" => ImportOrigin::SelfModule,
            "super" => ImportOrigin::ParentModule,
            "crate" => ImportOrigin::Crate,
            _ => ImportOrigin::External,
        };

        Self { origin, path }
    }

    fn tree_path(tree: &UseTree) -> String {
        match tree {
            UseTree::Path(path) => format!("{}::{}", path.ident, Self::tree_path(&path.tree)),
            UseTree::Name(name) => name.ident.to_string(),
            UseTree::Rename(rename) => format!("{} as {}", rename.ident, rename.rename),
            UseTree::Glob(_) => "*".to_owned(),
            UseTree::Group(group) => group.items.iter().map(Self::tree_path).join(","),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ImportOrigin {
    StandardLibrary,
    External,
    SelfModule,
    ParentModule,
    Crate,
}

impl ImportOrigin {
    const fn group(self) -> ImportGroup {
        match self {
            Self::StandardLibrary => ImportGroup::StandardLibrary,
            Self::External => ImportGroup::External,
            Self::SelfModule | Self::ParentModule | Self::Crate => ImportGroup::Local,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
enum ImportGroup {
    #[display("standard-library")]
    StandardLibrary,
    #[display("external")]
    External,
    #[display("local")]
    Local,
}

#[cfg(test)]
mod tests {
    use anyhow::Context;

    use super::*;
    use crate::config::{Config, ItemClass, ItemKind, Visibility};

    fn default_config() -> Result<Config> {
        Config::parse(
            r#"
            order = [
                "use",
                "pub(crate) use",
                "pub use",
                "mod",
                "pub(crate) mod",
                "pub mod",
            ]
            "#,
        )
        .context("default config")
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
            pub(crate) mod e;
            pub mod f;
            "#,
            &default_config()?,
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
            &default_config()?,
        )?;

        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].kind,
            ViolationKind::ItemOrder {
                found: ItemClass::new(ItemKind::Use, Visibility::Crate),
                must_precede: ItemClass::new(ItemKind::Use, Visibility::Public),
            }
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

            use crate::config::{Config, ItemClass, ItemKind, Visibility};

            use tracing::info;
            "#,
            &default_config()?,
        )?;

        assert_eq!(violations.len(), 1);
        Ok(())
    }

    #[test]
    fn reports_imports_that_are_not_alphabetical() -> Result<()> {
        let violations = check_source(
            Path::new("lib.rs"),
            r#"
            use tracing::info;
            use anyhow::Result;
            "#,
            &default_config()?,
        )?;

        assert_eq!(violations.len(), 1);
        Ok(())
    }

    #[test]
    fn accepts_imports_ordered_by_origin_and_path() -> Result<()> {
        let violations = check_source(
            Path::new("lib.rs"),
            r#"
            use std::path::Path;

            use anyhow::Result;
            use tracing::info;

            use self::local::Local;
            use super::parent::Parent;
            use crate::config::Config;
            "#,
            &default_config()?,
        )?;

        assert!(violations.is_empty());
        Ok(())
    }

    #[test]
    fn accepts_self_then_super_then_crate() -> Result<()> {
        let violations = check_source(
            Path::new("lib.rs"),
            r#"
            use self::local::*;
            use super::*;
            use crate::config::{Config, ItemClass, ItemKind, Visibility};
            "#,
            &default_config()?,
        )?;

        assert!(violations.is_empty());
        Ok(())
    }

    #[test]
    fn requires_blank_line_between_external_and_local_imports() -> Result<()> {
        let violations = check_source(
            Path::new("lib.rs"),
            r#"
            use anyhow::Context;
            use super::*;
            use crate::config::{Config, ItemClass, ItemKind, Visibility};
            "#,
            &default_config()?,
        )?;

        assert_eq!(violations.len(), 1);
        assert!(matches!(
            violations[0].kind,
            ViolationKind::MissingBlankLine { .. }
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
            &default_config()?,
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
            &default_config()?,
        )?;

        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].module_path,
            vec!["outer".to_owned(), "inner".to_owned()]
        );
        Ok(())
    }

    #[test]
    fn omitted_categories_are_run_boundaries() -> Result<()> {
        let config = Config::parse(
            r#"
            order = ["use", "pub use"]
            "#,
        )?;
        let violations = check_source(
            Path::new("lib.rs"),
            r#"
            pub use crate::a::A;
            mod boundary;
            use crate::b::B;
            "#,
            &config,
        )?;

        assert!(violations.is_empty());
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
            &default_config()?,
        )?;

        assert!(violations.is_empty());
        Ok(())
    }
}
