use std::path::{Path, PathBuf};

use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "cargo internal-checks",
    bin_name = "cargo internal-checks",
    version,
    about = "Checks the team's Rust source-layout conventions"
)]
pub(crate) struct Cli {
    /// Packages to check. If omitted, all workspace packages are checked.
    #[arg(short = 'p', long = "package", value_name = "NAME")]
    packages: Vec<String>,

    /// Path to Cargo.toml. Defaults to Cargo's normal manifest discovery.
    #[arg(long, value_name = "PATH")]
    manifest_path: Option<PathBuf>,

    /// Rust files or directories to check. May be repeated.
    #[arg(long = "path", value_name = "PATH")]
    paths: Vec<PathBuf>,

    /// Check only files changed in Git, including untracked files.
    #[arg(long)]
    changed: bool,

    /// Check files changed since REV diverged from HEAD, including local
    /// changes.
    #[arg(long, value_name = "REV", conflicts_with = "changed")]
    changed_since: Option<String>,

    /// Rewrite imports, module declarations, and error variants.
    /// Run the project's rustfmt afterward.
    #[arg(long)]
    fix: bool,

    /// Display absolute paths in diagnostics.
    #[arg(short, long)]
    verbose: bool,
}

impl Cli {
    pub(crate) fn packages(&self) -> &[String] {
        &self.packages
    }

    pub(crate) fn manifest_path(&self) -> Option<&Path> {
        self.manifest_path.as_deref()
    }

    pub(crate) fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    pub(crate) fn changed(&self) -> bool {
        self.changed
    }

    pub(crate) fn changed_since(&self) -> Option<&str> {
        self.changed_since.as_deref()
    }

    pub(crate) fn fix(&self) -> bool {
        self.fix
    }

    pub(crate) fn verbose(&self) -> bool {
        self.verbose
    }
}
