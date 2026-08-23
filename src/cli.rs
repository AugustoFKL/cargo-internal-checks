use std::path::{Path, PathBuf};

use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "cargo internal-checks",
    bin_name = "cargo internal-checks",
    version,
    about = "Checks configurable ordering of Rust module items"
)]
pub(crate) struct Cli {
    /// Packages to check. If omitted, all workspace packages are checked.
    #[arg(short = 'p', long = "package", value_name = "NAME")]
    packages: Vec<String>,

    /// Path to Cargo.toml. Defaults to Cargo's normal manifest discovery.
    #[arg(long, value_name = "PATH")]
    manifest_path: Option<PathBuf>,

    /// Path to the internal-checks configuration.
    /// Defaults to <workspace-root>/internal-checks.toml.
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,

    /// Rewrite imports, module declarations, and error variants.
    /// Run the project's rustfmt afterward.
    #[arg(long)]
    fix: bool,
}

impl Cli {
    pub(crate) fn packages(&self) -> &[String] {
        &self.packages
    }

    pub(crate) fn manifest_path(&self) -> Option<&Path> {
        self.manifest_path.as_deref()
    }

    pub(crate) fn config(&self) -> Option<&PathBuf> {
        self.config.as_ref()
    }

    pub(crate) fn fix(&self) -> bool {
        self.fix
    }
}
