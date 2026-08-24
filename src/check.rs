use std::{fs, path::Path};

use anyhow::{Context, Result};

use crate::{diagnostic::Violation, rules, source::Source};

pub(crate) fn check_file(path: &Path) -> Result<Vec<Violation>> {
    let source = fs::read_to_string(path)
        .with_context(|| format!("failed to read Rust source `{path:#?}`"))?;
    check_source(path, &source)
}

fn check_source(path: &Path, source: &str) -> Result<Vec<Violation>> {
    let file = syn::parse_file(source)
        .with_context(|| format!("failed to parse Rust source `{path:#?}`"))?;
    let source = Source::new(source);
    let mut violations = Vec::new();

    rules::item_order::check(path, &source, &file.items, &mut violations);
    rules::error_variants::check(path, &source, &file.items, &mut violations);

    Ok(violations)
}
