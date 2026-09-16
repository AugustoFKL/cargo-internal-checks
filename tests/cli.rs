//! End-to-end tests for command-line behavior and filesystem orchestration.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT_PROJECT: AtomicUsize = AtomicUsize::new(0);

struct TestProject {
    root: PathBuf,
}

impl TestProject {
    fn new(source: &str) -> std::io::Result<Self> {
        let id = NEXT_PROJECT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "cargo-internal-checks-cli-tests-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("src"))?;
        fs::write(
            root.join("Cargo.toml"),
            r#"[package]
name = "fixture"
version = "0.1.0"
edition = "2024"

[workspace]
"#,
        )?;
        fs::write(root.join("src/lib.rs"), source)?;

        Ok(Self { root })
    }

    fn source_path(&self) -> PathBuf {
        self.root.join("src").join("lib.rs")
    }

    fn write(&self, path: impl AsRef<Path>, contents: &str) -> std::io::Result<()> {
        let path = self.root.join(path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, contents)
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_cargo-internal-checks"));
        command
            .current_dir(&self.root)
            .arg("--manifest-path")
            .arg(self.root.join("Cargo.toml"));
        command
    }

    fn run(&self, arguments: &[&str]) -> std::io::Result<Output> {
        self.command().args(arguments).output()
    }

    fn git(&self, arguments: &[&str]) -> std::io::Result<Output> {
        Command::new("git")
            .current_dir(&self.root)
            .args(arguments)
            .output()
    }

    fn run_git(&self, arguments: &[&str]) -> std::io::Result<()> {
        let output = self.git(arguments)?;
        if output.status.success() {
            return Ok(());
        }

        Err(std::io::Error::other(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ))
    }

    fn initialize_git(&self) -> std::io::Result<()> {
        self.run_git(&["init", "--quiet"])?;
        self.run_git(&["add", "."])?;
        self.run_git(&[
            "-c",
            "user.name=cargo-internal-checks",
            "-c",
            "user.email=cargo-internal-checks@example.invalid",
            "commit",
            "--quiet",
            "--no-gpg-sign",
            "--no-verify",
            "-m",
            "initial fixture",
        ])
    }
}

impl Drop for TestProject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn reports_violations_and_fixes_them_idempotently() -> std::io::Result<()> {
    let source = r#"mod errors {
    #[derive(Error)]
    enum Error {
        Second,
        First,
    }
}
use std::path::Path;
"#;
    let expected = r#"use std::path::Path;

mod errors {
    #[derive(Error)]
    enum Error {
        First,

        Second,
    }
}
"#;
    let project = TestProject::new(source)?;

    let output = project.run(&[])?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    let relative_source = Path::new("src").join("lib.rs");
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr.starts_with(&format!("{}:", relative_source.display())));
    assert!(!stderr.contains(&project.root.display().to_string()));
    assert!(stderr.contains("`use` must appear before `mod`"));
    assert!(stderr.contains("error variant `First` must appear before `Second`"));
    assert!(stderr.contains("module: errors"));

    let output = project.run(&["--verbose"])?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr.starts_with(&format!("{}:", project.source_path().display())),
        "unexpected verbose diagnostics: {stderr}"
    );

    let output = project.run(&["--fix"])?;
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(fs::read_to_string(project.source_path())?, expected);

    let output = project.run(&["--fix"])?;
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(fs::read_to_string(project.source_path())?, expected);
    Ok(())
}

#[test]
fn accepts_package_option_without_configuration() -> std::io::Result<()> {
    let project = TestProject::new("use std::path::Path;\n")?;

    let output = project.run(&["--package", "fixture"])?;
    assert_eq!(output.status.code(), Some(0));
    Ok(())
}

#[test]
fn limits_checking_and_fixing_to_selected_paths() -> std::io::Result<()> {
    let project = TestProject::new("pub struct Valid;\n")?;
    let invalid = "mod errors;\nuse std::path::Path;\n";
    let fixed = "use std::path::Path;\n\nmod errors;\n";
    project.write("src/selected.rs", invalid)?;
    project.write("src/unselected.rs", invalid)?;

    let output = project.run(&["--path", "src/selected.rs"])?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr.contains(&Path::new("src").join("selected.rs").display().to_string()));
    assert!(!stderr.contains(&Path::new("src").join("unselected.rs").display().to_string()));

    let output = project.run(&["--fix", "--path", "src/selected.rs"])?;
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        fs::read_to_string(project.root.join("src/selected.rs"))?,
        fixed
    );
    assert_eq!(
        fs::read_to_string(project.root.join("src/unselected.rs"))?,
        invalid
    );
    Ok(())
}

#[test]
fn accepts_repeated_file_and_directory_paths() -> std::io::Result<()> {
    let project = TestProject::new("pub struct Valid;\n")?;
    project.write("src/direct.rs", "use crate::a::A;\n")?;
    project.write(
        "src/nested/invalid.rs",
        "use crate::a::A;\n\nuse crate::b::B;\n",
    )?;
    project.write("src/excluded.rs", "fn invalid(")?;

    let output = project.run(&["--path", "src/direct.rs", "--path", "src/nested"])?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr.contains(
            &Path::new("src")
                .join("nested")
                .join("invalid.rs")
                .display()
                .to_string()
        )
    );
    assert!(!stderr.contains("failed to parse Rust source"));
    Ok(())
}

#[test]
fn selects_local_git_changes_and_fixes_only_existing_changed_files() -> std::io::Result<()> {
    let project = TestProject::new("pub struct Valid;\n")?;
    let invalid = "mod errors;\nuse std::path::Path;\n";
    let fixed = "use std::path::Path;\n\nmod errors;\n";
    for path in [
        "src/staged.rs",
        "src/unstaged.rs",
        "src/unchanged.rs",
        "src/deleted.rs",
        "src/renamed_old.rs",
    ] {
        project.write(path, invalid)?;
    }
    project.write("ignored.rs", invalid)?;
    project.write(".gitignore", "ignored.rs\n")?;
    project.initialize_git()?;

    project.write("src/staged.rs", "mod errors;\n\nuse std::path::Path;\n")?;
    project.run_git(&["add", "src/staged.rs"])?;
    project.write("src/unstaged.rs", "mod errors;\n\n\nuse std::path::Path;\n")?;
    project.write("src/untracked.rs", invalid)?;
    fs::remove_file(project.root.join("src/deleted.rs"))?;
    fs::rename(
        project.root.join("src/renamed_old.rs"),
        project.root.join("src/renamed_new.rs"),
    )?;

    let output = project.run(&["--changed"])?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1));
    for name in ["staged.rs", "unstaged.rs", "untracked.rs", "renamed_new.rs"] {
        let path = Path::new("src").join(name);
        assert!(
            stderr.contains(&path.display().to_string()),
            "missing changed file `{}` in diagnostics: {stderr}",
            path.display()
        );
    }
    for name in ["unchanged.rs", "deleted.rs", "renamed_old.rs"] {
        let path = Path::new("src").join(name);
        assert!(
            !stderr.contains(&path.display().to_string()),
            "unexpected file `{}` in diagnostics: {stderr}",
            path.display()
        );
    }
    assert!(!stderr.contains("ignored.rs"));

    let output = project.run(&["--changed", "--path", "src/staged.rs"])?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr.contains(&Path::new("src").join("staged.rs").display().to_string()));
    assert!(!stderr.contains(&Path::new("src").join("unstaged.rs").display().to_string()));

    let output = project.run(&["--fix", "--changed"])?;
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        fs::read_to_string(project.root.join("src/staged.rs"))?,
        fixed
    );
    for path in ["src/unstaged.rs", "src/untracked.rs", "src/renamed_new.rs"] {
        assert_eq!(fs::read_to_string(project.root.join(path))?, fixed);
    }
    assert_eq!(
        fs::read_to_string(project.root.join("src/unchanged.rs"))?,
        invalid
    );
    assert_eq!(
        fs::read_to_string(project.root.join("ignored.rs"))?,
        invalid
    );
    Ok(())
}

#[test]
fn accepts_an_empty_changed_file_selection() -> std::io::Result<()> {
    let project = TestProject::new("pub struct Valid;\n")?;
    project.initialize_git()?;

    let output = project.run(&["--changed"])?;
    assert_eq!(output.status.code(), Some(0));
    Ok(())
}

#[test]
fn reports_changed_selection_outside_a_git_repository() -> std::io::Result<()> {
    let project = TestProject::new("pub struct Valid;\n")?;

    let output = project.run(&["--changed"])?;
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("failed to locate the Git repository")
    );
    Ok(())
}

#[test]
fn runs_without_configuration_and_reports_invalid_rust() -> std::io::Result<()> {
    let project = TestProject::new("pub struct Valid;\n")?;

    let output = project.run(&[])?;
    assert_eq!(output.status.code(), Some(0));

    project.write("src/lib.rs", "fn invalid(")?;
    let output = project.run(&[])?;
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("failed to parse Rust source"));
    Ok(())
}
