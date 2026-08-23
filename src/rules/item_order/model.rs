use std::collections::HashSet;

use derive_more::Display;
use itertools::Itertools;
use proc_macro2::Span;
use syn::{Item, UseTree};

use crate::config::{Config, ItemClass, ItemKind, Visibility};

#[derive(Debug)]
pub(super) struct ClassifiedItem {
    pub(super) class: ItemClass,
    pub(super) span: Span,
    pub(super) end_line: usize,
    pub(super) import_key: Option<ImportKey>,
}

impl ClassifiedItem {
    pub(super) fn from_ast(item: &Item, scope: &ModuleScope) -> Option<Self> {
        match item {
            Item::Use(item) => Some(Self {
                class: ItemClass::new(ItemKind::Use, Self::visibility(&item.vis)?),
                span: item.use_token.span,
                end_line: item.semi_token.span.end().line,
                import_key: Some(ImportKey::from_tree(&item.tree, scope)),
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

    pub(super) fn group(&self) -> ItemGroup {
        self.import_key
            .as_ref()
            .map_or(ItemGroup::Module, |key| key.group)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ImportKey {
    pub(super) group: ItemGroup,
    pub(super) path: String,
}

impl ImportKey {
    fn from_tree(tree: &UseTree, scope: &ModuleScope) -> Self {
        let path = Self::tree_path(tree);
        let root = path.split("::").next().unwrap_or_default();
        let group = Self::classify_group(root, scope);

        Self { group, path }
    }

    fn classify_group(root: &str, scope: &ModuleScope) -> ItemGroup {
        if scope.contains_module(root) {
            return ItemGroup::Local;
        }

        match root {
            "std" | "core" | "alloc" => ItemGroup::StandardLibrary,
            "self" | "super" | "crate" => ItemGroup::Local,
            _ => ItemGroup::External,
        }
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

#[derive(Debug, Default)]
pub(super) struct ModuleScope {
    local_modules: HashSet<String>,
}

impl ModuleScope {
    pub(super) fn from_items(items: &[Item]) -> Self {
        let mut scope = Self::default();

        for item in items {
            if let Item::Mod(item_mod) = item {
                scope.local_modules.insert(item_mod.ident.to_string());
            }
        }

        scope
    }

    fn contains_module(&self, name: &str) -> bool {
        self.local_modules.contains(name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Display)]
pub(crate) enum ItemGroup {
    #[display("standard-library")]
    StandardLibrary,
    #[display("external")]
    External,
    #[display("local")]
    Local,
    #[display("module")]
    Module,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ItemPlacement {
    rank: usize,
    group: ItemGroup,
    class: ItemClass,
}

impl ItemPlacement {
    pub(super) fn from_ast(item: &Item, config: &Config, scope: &ModuleScope) -> Option<Self> {
        let classified = ClassifiedItem::from_ast(item, scope)?;
        let rank = config.rank(classified.class)?;
        let group = classified.group();

        Some(Self {
            rank,
            group,
            class: classified.class,
        })
    }

    pub(super) const fn sort_key(self) -> (usize, ItemGroup) {
        (self.rank, self.group)
    }

    pub(super) fn starts_new_group_after(self, previous: Self) -> bool {
        self.class != previous.class || self.group != previous.group
    }
}
