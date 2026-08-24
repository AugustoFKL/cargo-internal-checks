//! Verification and formatting of Rust import order.
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    process::ExitCode,
};

use anyhow::{Result, bail};
use clap::Parser;
use tracing::{error, info};

use crate::{cli::Cli, config::Config, diagnostic::Violation, project::Project};

mod check;
mod cli;
mod config;
mod diagnostic;
mod edit;
mod fix;
mod logging;
mod project;
mod rules;
mod source;

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(error) => {
            error!("error: {error:#}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<bool> {
    logging::setup()?;
    info!("logging setup");

    let args = Cli::parse();
    let project = Project::discover(args.manifest_path(), args.packages())?;
    let config_path = resolve_config_path(args.config().cloned(), project.workspace_root())?;
    let config = Config::load(&config_path)?;
    let files = project.rust_files()?;

    if args.fix() {
        let mut fixed = 0;
        for path in &files {
            fixed += usize::from(fix::fix_file(path, &config)?);
        }
        info!("internal-checks: fixed {fixed} Rust file(s)");
        if fixed > 0 {
            info!("internal-checks: run the project's rustfmt to format imports within groups");
        }
    }

    let mut violations = Vec::new();
    for path in &files {
        violations.extend(check::check_file(path, &config)?);
    }

    if violations.is_empty() {
        info!(
            "internal-checks: checked {} Rust file(s); no violations",
            files.len()
        );
        return Ok(true);
    }

    for violation in &violations {
        print_violation(violation, project.workspace_root(), args.verbose());
    }

    let affected_files: BTreeSet<_> = violations
        .iter()
        .map(|violation| violation.path())
        .collect();

    error!(
        "internal-checks: found {} violation(s) in {} file(s)",
        violations.len(),
        affected_files.len()
    );
    Ok(false)
}

fn resolve_config_path(config: Option<PathBuf>, workspace_root: &Path) -> Result<PathBuf> {
    let path = config.unwrap_or_else(|| workspace_root.join("internal-checks.toml"));
    if !path.is_file() {
        bail!("configuration file `{path:#?}` does not exist; create it or pass `--config <PATH>`",);
    }
    Ok(path)
}

fn print_violation(violation: &Violation, workspace_root: &Path, verbose: bool) {
    let path = if verbose {
        violation.path()
    } else {
        violation
            .path()
            .strip_prefix(workspace_root)
            .unwrap_or(violation.path())
    };

    eprintln!(
        "{}:{}:{}: error: {violation}",
        path.display(),
        violation.line(),
        violation.column()
    );
    if !violation.module_path().is_empty() {
        eprintln!("  module: {}", violation.module_path().join("::"));
    };
}
