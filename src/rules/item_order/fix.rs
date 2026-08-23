use syn::{Item, ItemMod, spanned::Spanned};

use super::model::{ItemPlacement, ModuleScope};
use crate::{config::Config, edit::Edit, source::Source};

pub(crate) fn edits(source: &Source<'_>, config: &Config, items: &[Item]) -> Vec<Edit> {
    let fixer = ItemOrderFixer::new(source, config);
    let mut edits = Vec::new();
    fixer.collect_edits_into(items, &mut edits);
    edits
}

struct ItemOrderFixer<'a, 'source> {
    source: &'a Source<'source>,
    config: &'a Config,
}

impl<'a, 'source> ItemOrderFixer<'a, 'source> {
    fn new(source: &'a Source<'source>, config: &'a Config) -> Self {
        Self { source, config }
    }

    fn collect_edits_into(&self, items: &[Item], edits: &mut Vec<Edit>) {
        let scope = ModuleScope::from_items(items);
        let mut index = 0;
        while index < items.len() {
            let run_start = index;
            let Some((run, next_index)) = self.item_run(items, index, &scope) else {
                self.collect_inline_module(&items[index], edits);
                index += 1;
                continue;
            };

            if let Some(edit) = self.edit_for_run(&run) {
                // Do not create an edit inside a module that this edit moves.
                // The engine reparses the moved module on the next pass.
                edits.push(edit);
            } else {
                for item in &items[run_start..next_index] {
                    self.collect_inline_module(item, edits);
                }
            }

            index = next_index;
        }
    }

    fn item_run(
        &self,
        items: &[Item],
        start: usize,
        scope: &ModuleScope,
    ) -> Option<(ItemRun, usize)> {
        let first = FixableItem::from_ast(&items[start], self.source, self.config, scope)?;
        let mut fixable_items = vec![first];
        let mut end = start + 1;

        while let Some(ast) = items.get(end) {
            let Some(item) = FixableItem::from_ast(ast, self.source, self.config, scope) else {
                break;
            };

            fixable_items.push(item);
            end += 1;
        }

        Some((
            ItemRun {
                items: fixable_items,
            },
            end,
        ))
    }

    fn edit_for_run(&self, run: &ItemRun) -> Option<Edit> {
        if run.len() < 2 || run.has_ambiguous_text(self.source.text()) {
            return None;
        }

        let mut sorted = run.items.clone();
        sorted.sort_by_key(|item| item.placement.sort_key());

        let indentation = self.source.indentation_at(run.start());
        let replacement = self.render_items(&sorted, indentation)?;
        let start = run.start();
        let end = run.end();

        if replacement == self.source.text()[start..end] {
            return None;
        }

        Some(Edit::new(start, end, replacement))
    }

    fn render_items(&self, items: &[FixableItem], indentation: &str) -> Option<String> {
        let first = items.first()?;
        let mut replacement = self.source.text()[first.start..first.end].to_owned();

        for pair in items.windows(2) {
            self.write_separator(&mut replacement, &pair[0], &pair[1], indentation);
            replacement.push_str(&self.source.text()[pair[1].start..pair[1].end]);
        }

        Some(replacement)
    }

    fn write_separator(
        &self,
        output: &mut String,
        previous: &FixableItem,
        current: &FixableItem,
        indentation: &str,
    ) {
        output.push_str(self.source.newline());
        if current.placement.starts_new_group_after(previous.placement) {
            output.push_str(self.source.newline());
        }
        output.push_str(indentation);
    }

    fn collect_inline_module(&self, item: &Item, edits: &mut Vec<Edit>) {
        let Item::Mod(ItemMod {
            content: Some((_, items)),
            ..
        }) = item
        else {
            return;
        };

        self.collect_edits_into(items, edits);
    }
}

struct ItemRun {
    items: Vec<FixableItem>,
}

impl ItemRun {
    fn len(&self) -> usize {
        self.items.len()
    }

    fn start(&self) -> usize {
        self.items[0].start
    }

    fn end(&self) -> usize {
        self.items[self.items.len() - 1].end
    }

    fn has_ambiguous_text(&self, source: &str) -> bool {
        for pair in self.items.windows(2) {
            if !source[pair[0].end..pair[1].start].trim().is_empty() {
                return true;
            }
        }

        false
    }
}

#[derive(Clone)]
struct FixableItem {
    start: usize,
    end: usize,
    placement: ItemPlacement,
}

impl FixableItem {
    fn from_ast(
        item: &Item,
        source: &Source<'_>,
        config: &Config,
        scope: &ModuleScope,
    ) -> Option<Self> {
        let span = item.span();

        Some(Self {
            start: source.offset(span.start())?,
            end: source.offset(span.end())?,
            placement: ItemPlacement::from_ast(item, config, scope)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Result;

    use crate::{config::Config, fix::fix_source};

    fn config() -> Result<Config> {
        Config::parse(
            r#"order = ["use", "pub(crate) use", "pub use", "mod", "pub(crate) mod", "pub mod"]"#,
        )
    }

    fn fixed(source: &str) -> Result<String> {
        fix_source(source, &config()?)
    }

    #[test]
    fn groups_imports_by_visibility_and_source() -> Result<()> {
        let fixed = fixed(
            "use crate::private::C;\npub use anyhow::A;\nuse anyhow::B;\nuse std::path::Path;\n",
        )?;

        assert_eq!(
            fixed,
            "use std::path::Path;\n\nuse anyhow::B;\n\nuse crate::private::C;\n\npub use anyhow::A;\n"
        );
        Ok(())
    }

    #[test]
    fn preserves_order_within_an_import_group() -> Result<()> {
        let fixed = fixed("use tracing::info;\nuse anyhow::Result;\n")?;

        assert_eq!(fixed, "use tracing::info;\nuse anyhow::Result;\n");
        Ok(())
    }

    #[test]
    fn preserves_rustfmt_order_within_the_local_group() -> Result<()> {
        let source = r#"pub use crate::{
    bindings::{cudaFree, cudaMalloc, cudaMemcpy, cudaMemcpy2D, cudaMemcpyAsync, cudaMemcpyKind},
    error::CudaError,
};
pub use async_device_buffer::AsyncDeviceBuffer;
pub use device_buffer::DeviceBuffer;

pub mod async_device_buffer;
pub mod device_buffer;
"#;

        assert_eq!(fixed(source)?, source);
        Ok(())
    }

    #[test]
    fn leaves_runs_with_comments_unchanged() -> Result<()> {
        let source = "use crate::local::A;\n// belongs to the next import\nuse anyhow::B;\n";

        assert_eq!(fixed(source)?, source);
        Ok(())
    }

    #[test]
    fn does_not_reorder_across_unrelated_items() -> Result<()> {
        let source =
            "pub use crate::public::A;\nconst BOUNDARY: usize = 1;\nuse std::path::Path;\n";

        assert_eq!(fixed(source)?, source);
        Ok(())
    }

    #[test]
    fn orders_module_declarations() -> Result<()> {
        let fixed = fixed("pub mod public;\npub(crate) mod internal;\nmod private;\n")?;

        assert_eq!(
            fixed,
            "mod private;\n\npub(crate) mod internal;\n\npub mod public;\n"
        );
        Ok(())
    }

    #[test]
    fn groups_interleaved_imports_and_modules() -> Result<()> {
        let fixed = fixed(
            r#"//! Verification and formatting of Rust import order.
mod cli;
use std::collections::BTreeSet;
use anyhow::Result;
use crate::check::Violation;
mod config;
use clap::Parser;
mod check;
use tracing::info;
mod fix;
mod logging;
mod project;
"#,
        )?;

        assert_eq!(
            fixed,
            r#"//! Verification and formatting of Rust import order.
use std::collections::BTreeSet;

use anyhow::Result;
use clap::Parser;
use tracing::info;

use crate::check::Violation;

mod cli;
mod config;
mod check;
mod fix;
mod logging;
mod project;
"#
        );
        Ok(())
    }

    #[test]
    fn preserves_alphabetical_reexports_of_local_modules_named_core() -> Result<()> {
        let source = r#"pub use arithmetic::*;
pub use core::*;
pub use encryption::*;
pub use helpers::*;
pub use keys::*;

mod arithmetic;
mod core;
mod encryption;
mod helpers;
mod keys;
"#;

        assert_eq!(fixed(source)?, source);
        Ok(())
    }

    #[test]
    fn moves_attributed_imports_with_their_attributes() -> Result<()> {
        let source = r#"use crate::local::Local;
#[cfg(feature = "optional")]
use anyhow::Result;
"#;

        assert_eq!(
            fixed(source)?,
            r#"#[cfg(feature = "optional")]
use anyhow::Result;

use crate::local::Local;
"#
        );
        Ok(())
    }
}
