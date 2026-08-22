//! Verification and formatting of Rust import order.
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    process::ExitCode,
};

use anyhow::{Result, bail};
use clap::Parser;
use tracing::{error, info};

use crate::{check::Violation, cli::Cli, config::Config, project::Project};

mod check;
mod cli;
mod config;
mod logging;
mod project;

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(error) => {
            eprintln!("error: {error:#}");
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

    let mut violations = Vec::new();
    for path in &files {
        violations.extend(check::check_file(path, &config)?);
    }

    if violations.is_empty() {
        println!(
            "item-order: checked {} Rust file(s); no violations",
            files.len()
        );
        return Ok(true);
    }

    for violation in &violations {
        print_violation(violation, project.workspace_root());
    }

    let affected_files: BTreeSet<_> = violations
        .iter()
        .map(|violation| violation.path())
        .collect();

    info!(
        "item-order: found {} violation(s) in {} file(s)",
        violations.len(),
        affected_files.len()
    );
    Ok(false)
}

fn resolve_config_path(config: Option<PathBuf>, workspace_root: &Path) -> Result<PathBuf> {
    let path = config.unwrap_or_else(|| workspace_root.join("item-order.toml"));
    if !path.is_file() {
        bail!("configuration file `{path:#?}` does not exist; create it or pass `--config <PATH>`",);
    }
    Ok(path)
}

fn print_violation(violation: &Violation, workspace_root: &Path) {
    let path = violation
        .path()
        .strip_prefix(workspace_root)
        .unwrap_or(violation.path());

    error!("error[item-order]: {violation}");
    error!(
        " --> {}:{}:{}",
        path.display(),
        violation.line(),
        violation.column()
    );
    if !violation.module_path().is_empty() {
        error!("  = module: {}", violation.module_path().join("::"));
    };
}
