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
    changed_files_in_worktree(&repository_root)
}

/// Returns files changed since `revision` diverged from `HEAD`, including local
/// changes.
pub(crate) fn changed_files_since(
    workspace_root: &Path,
    revision: &str,
) -> Result<BTreeSet<PathBuf>> {
    let repository_root = repository_root(workspace_root)?;
    let mut changed = changed_files_in_worktree(&repository_root)?;
    let commit = resolve_commit(&repository_root, revision)?;
    let merge_base = merge_base(&repository_root, revision, &commit)?;
    let output = Command::new("git")
        .arg("-C")
        .arg(&repository_root)
        .args(["diff", "--name-only", "-z", "--find-renames"])
        .arg(merge_base)
        .arg("HEAD")
        .arg("--")
        .output()
        .context("failed to run Git while finding files changed since a revision")?;
    let context = format!("failed to find files changed since Git revision `{revision}`");
    let stdout = ensure_success(output, &context)?;
    changed.extend(canonical_existing_files(
        &repository_root,
        parse_diff_paths(&stdout)?,
    )?);

    Ok(changed)
}

fn changed_files_in_worktree(repository_root: &Path) -> Result<BTreeSet<PathBuf>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository_root)
        .args(["status", "--porcelain=v1", "-z", "--untracked-files=all"])
        .output()
        .context("failed to run Git while finding changed files")?;
    let stdout = ensure_success(output, "failed to find changed files with Git")?;
    canonical_existing_files(repository_root, parse_status(&stdout)?)
}

fn canonical_existing_files(
    repository_root: &Path,
    paths: Vec<PathBuf>,
) -> Result<BTreeSet<PathBuf>> {
    let mut files = BTreeSet::new();
    for path in paths {
        let path = repository_root.join(path);
        if !path.is_file() {
            continue;
        }

        files.insert(
            fs::canonicalize(&path)
                .with_context(|| format!("failed to resolve changed file `{}`", path.display()))?,
        );
    }

    Ok(files)
}

fn resolve_commit(repository_root: &Path, revision: &str) -> Result<String> {
    let expression = format!("{revision}^{{commit}}");
    let output = Command::new("git")
        .arg("-C")
        .arg(repository_root)
        .args(["rev-parse", "--verify", "--end-of-options"])
        .arg(expression)
        .output()
        .context("failed to run Git while resolving a revision")?;
    let context = format!("failed to resolve Git revision `{revision}` to a commit");
    let stdout = ensure_success(output, &context)?;
    parse_output_line(&stdout, "resolved commit")
}

fn merge_base(repository_root: &Path, revision: &str, commit: &str) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository_root)
        .args(["merge-base", commit, "HEAD"])
        .output()
        .context("failed to run Git while finding a merge base")?;
    let context = format!("failed to find a merge base between Git revision `{revision}` and HEAD");
    let stdout = ensure_success(output, &context)?;
    parse_output_line(&stdout, "merge base")
}

fn parse_output_line(output: &[u8], description: &str) -> Result<String> {
    let value = std::str::from_utf8(output)
        .with_context(|| format!("Git returned a {description} that is not UTF-8"))?
        .trim_end_matches(['\r', '\n']);
    if value.is_empty() || value.contains(['\r', '\n']) {
        bail!("Git returned an invalid {description}");
    }

    Ok(value.to_owned())
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

fn parse_diff_paths(output: &[u8]) -> Result<Vec<PathBuf>> {
    let mut records = output.split(|byte| *byte == 0).peekable();
    let mut paths = Vec::new();

    while let Some(record) = records.next() {
        if record.is_empty() && records.peek().is_none() {
            break;
        }
        if record.is_empty() {
            bail!("Git returned an empty changed path");
        }

        let path =
            std::str::from_utf8(record).context("Git returned a changed path that is not UTF-8")?;
        paths.push(PathBuf::from(path));
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

    #[test]
    fn parses_nul_delimited_diff_paths() -> Result<()> {
        assert_eq!(
            parse_diff_paths(b"src/first.rs\0src/with space.rs\0")?,
            [
                PathBuf::from("src/first.rs"),
                PathBuf::from("src/with space.rs")
            ]
        );
        Ok(())
    }

    #[test]
    fn rejects_empty_diff_paths() {
        assert!(parse_diff_paths(b"src/first.rs\0\0src/second.rs\0").is_err());
    }
}
