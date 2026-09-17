//! Verification and formatting of Rust import order.
use std::{collections::BTreeSet, path::Path, process::ExitCode};

use anyhow::Result;
use clap::Parser;

use crate::{cli::Cli, diagnostic::Violation, project::Project};

mod check;
mod cli;
mod diagnostic;
mod edit;
mod fix;
mod git;
mod project;
mod rules;
mod source;

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
    let args = Cli::parse();
    let project = Project::discover(args.manifest_path(), args.packages())?;
    let files = project.rust_files(args.paths(), args.changed(), args.changed_since())?;

    if args.fix() {
        let mut fixed = 0;
        for path in &files {
            fixed += usize::from(fix::fix_file(path)?);
        }
        eprintln!("internal-checks: fixed {fixed} Rust file(s)");
        if fixed > 0 {
            eprintln!("internal-checks: run the project's rustfmt to format imports within groups");
        }
    }

    let mut violations = Vec::new();
    for path in &files {
        violations.extend(check::check_file(path)?);
    }

    if violations.is_empty() {
        eprintln!(
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

    eprintln!(
        "internal-checks: found {} violation(s) in {} file(s)",
        violations.len(),
        affected_files.len()
    );
    Ok(false)
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
