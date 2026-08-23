use std::{fs, path::Path};

use anyhow::{Context, Result};
use syn::Item;

use crate::{config::Config, edit::Edit, rules, source::Source};

pub(crate) fn fix_file(path: &Path, config: &Config) -> Result<bool> {
    let source = fs::read_to_string(path)
        .with_context(|| format!("failed to read Rust source `{path:#?}`"))?;
    let fixed = fix_source(&source, config)
        .with_context(|| format!("failed to parse Rust source `{path:#?}`"))?;
    if fixed == source {
        return Ok(false);
    }

    fs::write(path, fixed).with_context(|| format!("failed to write Rust source `{path:#?}`"))?;
    Ok(true)
}

pub(crate) fn fix_source(source: &str, config: &Config) -> Result<String> {
    let mut fixed = source.to_owned();

    loop {
        let file = syn::parse_file(&fixed)?;
        let source = Source::new(&fixed);
        let edits = next_edits(&source, config, &file.items);

        if edits.is_empty() {
            return Ok(fixed);
        }

        fixed = Edit::apply_all(fixed, edits);
    }
}

fn next_edits(source: &Source<'_>, config: &Config, items: &[Item]) -> Vec<Edit> {
    let item_edits = rules::item_order::edits(source, config, items);
    if !item_edits.is_empty() {
        return item_edits;
    }

    rules::error_variants::edits(source, items)
}
