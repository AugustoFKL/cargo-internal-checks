use std::path::{Path, PathBuf};

use derive_more::Display;
use proc_macro2::{LineColumn, Span};

use crate::rules;

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
    pub(crate) fn at(path: &Path, span: Span, module_path: &[String], kind: ViolationKind) -> Self {
        let LineColumn { line, column } = span.start();
        Self {
            path: path.to_path_buf(),
            line,
            column: column + 1,
            module_path: module_path.to_vec(),
            kind,
        }
    }

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

    #[cfg(test)]
    pub(crate) fn kind(&self) -> &ViolationKind {
        &self.kind
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Display)]
pub(crate) enum ViolationKind {
    #[display("{_0}")]
    ItemOrder(rules::item_order::Violation),
    #[display("{_0}")]
    ErrorVariants(rules::error_variants::Violation),
}
