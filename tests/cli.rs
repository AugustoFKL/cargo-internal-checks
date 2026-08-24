//! End-to-end tests for command-line behavior and filesystem orchestration.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

const CONFIG: &str = r#"order = [
    "use",
    "pub(crate) use",
    "pub use",
    "mod",
    "pub(crate) mod",
    "pub mod",
]
"#;

static NEXT_PROJECT: AtomicUsize = AtomicUsize::new(0);

struct TestProject {
    root: PathBuf,
}

impl TestProject {
    fn new(source: &str, default_config: bool) -> std::io::Result<Self> {
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
        if default_config {
            fs::write(root.join("internal-checks.toml"), CONFIG)?;
        }

        Ok(Self { root })
    }

    fn source_path(&self) -> PathBuf {
        self.root.join("src").join("lib.rs")
    }

    fn write(&self, path: impl AsRef<Path>, contents: &str) -> std::io::Result<()> {
        fs::write(self.root.join(path), contents)
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
    let project = TestProject::new(source, true)?;

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
fn accepts_package_and_explicit_config_options() -> std::io::Result<()> {
    let project = TestProject::new("use std::path::Path;\n", false)?;
    project.write("custom.toml", r#"order = ["use"]"#)?;

    let output = project.run(&["--package", "fixture", "--config", "custom.toml"])?;
    assert_eq!(output.status.code(), Some(0));
    Ok(())
}

#[test]
fn reports_missing_configuration_and_invalid_rust() -> std::io::Result<()> {
    let project = TestProject::new("pub struct Valid;\n", false)?;

    let output = project.run(&[])?;
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stdout).contains("does not exist"));

    project.write("internal-checks.toml", CONFIG)?;
    project.write("src/lib.rs", "fn invalid(")?;
    let output = project.run(&[])?;
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stdout).contains("failed to parse Rust source"));
    Ok(())
}
