use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use anyhow::{Context, Result, bail};

/// Returns the canonical paths changed in the Git worktree or index.
pub(crate) fn changed_files(workspace_root: &Path) -> Result<BTreeSet<PathBuf>> {
    let repository_root = repository_root(workspace_root)?;
    let output = Command::new("git")
        .arg("-C")
        .arg(&repository_root)
        .args(["status", "--porcelain=v1", "-z", "--untracked-files=all"])
        .output()
        .context("failed to run Git while finding changed files")?;
    let stdout = ensure_success(output, "failed to find changed files with Git")?;
    let mut changed = BTreeSet::new();

    for path in parse_status(&stdout)? {
        let path = repository_root.join(path);
        if !path.is_file() {
            continue;
        }

        changed.insert(
            fs::canonicalize(&path)
                .with_context(|| format!("failed to resolve changed file `{}`", path.display()))?,
        );
    }

    Ok(changed)
}

fn repository_root(workspace_root: &Path) -> Result<PathBuf> {
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace_root)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("failed to run Git while locating the repository")?;
    let stdout = ensure_success(output, "failed to locate the Git repository")?;
    let root = std::str::from_utf8(&stdout)
        .context("Git returned a repository path that is not UTF-8")?
        .trim_end_matches(['\r', '\n']);
    if root.is_empty() {
        bail!("Git returned an empty repository path");
    }

    Ok(PathBuf::from(root))
}

fn ensure_success(output: Output, context: &str) -> Result<Vec<u8>> {
    if output.status.success() {
        return Ok(output.stdout);
    }

    let message = String::from_utf8_lossy(&output.stderr);
    let message = message.trim();
    if message.is_empty() {
        bail!("{context}");
    }
    bail!("{context}: {message}")
}

fn parse_status(output: &[u8]) -> Result<Vec<PathBuf>> {
    let mut records = output.split(|byte| *byte == 0).peekable();
    let mut paths = Vec::new();

    while let Some(record) = records.next() {
        if record.is_empty() && records.peek().is_none() {
            break;
        }
        if record.len() < 4 || record[2] != b' ' {
            bail!("Git returned malformed status output");
        }

        let path = std::str::from_utf8(&record[3..])
            .context("Git returned a changed path that is not UTF-8")?;
        paths.push(PathBuf::from(path));

        if matches!(record[0], b'R' | b'C') || matches!(record[1], b'R' | b'C') {
            let original = records
                .next()
                .context("Git omitted the original path for a rename or copy")?;
            if original.is_empty() {
                bail!("Git returned an empty original path for a rename or copy");
            }
        }
    }

    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modified_untracked_and_renamed_paths() -> Result<()> {
        let output = b" M src/modified.rs\0?? src/untracked file.rs\0R  src/new.rs\0src/old.rs\0 D src/deleted.rs\0";

        assert_eq!(
            parse_status(output)?,
            [
                PathBuf::from("src/modified.rs"),
                PathBuf::from("src/untracked file.rs"),
                PathBuf::from("src/new.rs"),
                PathBuf::from("src/deleted.rs"),
            ]
        );
        Ok(())
    }

    #[test]
    fn rejects_malformed_status_output() {
        assert!(parse_status(b"M src/missing-column.rs\0").is_err());
        assert!(parse_status(b"R  src/new.rs\0").is_err());
    }
}
