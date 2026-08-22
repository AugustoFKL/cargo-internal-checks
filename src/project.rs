use std::{
    collections::BTreeSet,
    ffi::OsStr,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use cargo_metadata::{MetadataCommand, Package};
use walkdir::{DirEntry, WalkDir};

/// The portion of a Cargo workspace selected for checking.
#[derive(Debug, Clone)]
pub(crate) struct Project {
    workspace_root: PathBuf,
    sources: SourceSet,
}

impl Project {
    pub(crate) fn discover(manifest_path: Option<&Path>, package_names: &[String]) -> Result<Self> {
        Workspace::load(manifest_path)?.select(package_names)
    }

    pub(crate) fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    pub(crate) fn rust_files(&self) -> Result<Vec<PathBuf>> {
        self.sources.rust_files()
    }
}

/// Cargo metadata normalized into the information this tool actually needs.
#[derive(Debug)]
struct Workspace {
    root: PathBuf,
    target_directory: PathBuf,
    packages: Vec<PackageRoot>,
}

impl Workspace {
    fn load(manifest_path: Option<&Path>) -> Result<Self> {
        let mut command = MetadataCommand::new();
        command.no_deps();
        if let Some(path) = manifest_path {
            command.manifest_path(path);
        }

        let metadata = command.exec().context("failed to load Cargo metadata")?;
        let packages = metadata
            .packages
            .iter()
            .filter(|package| metadata.workspace_members.contains(&package.id))
            .map(PackageRoot::from_metadata)
            .collect::<Result<_>>()?;

        Ok(Self {
            root: metadata.workspace_root.into_std_path_buf(),
            target_directory: metadata.target_directory.into_std_path_buf(),
            packages,
        })
    }

    fn select(self, requested_names: &[String]) -> Result<Project> {
        let selected_packages = self.selected_packages(requested_names)?;
        let workspace_package_roots = self
            .packages
            .iter()
            .map(|package| package.root.clone())
            .collect();

        Ok(Project {
            workspace_root: self.root,
            sources: SourceSet {
                packages: selected_packages,
                target_directory: self.target_directory,
                workspace_package_roots,
            },
        })
    }

    fn selected_packages(&self, requested_names: &[String]) -> Result<Vec<PackageRoot>> {
        if requested_names.is_empty() {
            return Ok(self.packages.clone());
        }

        requested_names
            .iter()
            .map(|requested| self.package_named(requested).cloned())
            .collect()
    }

    fn package_named(&self, requested: &str) -> Result<&PackageRoot> {
        let mut matches = self
            .packages
            .iter()
            .filter(|package| package.name == requested);

        let package = matches
            .next()
            .with_context(|| format!("package `{requested}` is not a workspace member"))?;
        if matches.next().is_some() {
            bail!("package name `{requested}` is ambiguous in this workspace");
        }

        Ok(package)
    }
}

#[derive(Debug, Clone)]
struct PackageRoot {
    name: String,
    root: PathBuf,
}

impl PackageRoot {
    fn from_metadata(package: &Package) -> Result<Self> {
        let root = package
            .manifest_path
            .parent()
            .context("package manifest has no parent directory")?
            .to_path_buf()
            .into_std_path_buf();

        Ok(Self {
            name: package.name.to_string(),
            root,
        })
    }
}

/// Filesystem state required to enumerate sources for the selected packages.
#[derive(Debug, Clone)]
struct SourceSet {
    packages: Vec<PackageRoot>,
    target_directory: PathBuf,
    workspace_package_roots: Vec<PathBuf>,
}

impl SourceSet {
    fn rust_files(&self) -> Result<Vec<PathBuf>> {
        let rules = TraversalRules {
            target_directory: &self.target_directory,
            workspace_package_roots: &self.workspace_package_roots,
        };
        let mut files = BTreeSet::new();

        for package in &self.packages {
            self.collect_package_files(package, &rules, &mut files)?;
        }

        Ok(files.into_iter().collect())
    }

    fn collect_package_files(
        &self,
        package: &PackageRoot,
        rules: &TraversalRules<'_>,
        files: &mut BTreeSet<PathBuf>,
    ) -> Result<()> {
        let entries = WalkDir::new(&package.root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| rules.should_visit(entry, &package.root));

        for entry in entries {
            let entry = entry
                .with_context(|| format!("failed while walking package `{}`", package.name))?;
            if is_rust_file(&entry) {
                files.insert(entry.into_path());
            }
        }

        Ok(())
    }
}

struct TraversalRules<'a> {
    target_directory: &'a Path,
    workspace_package_roots: &'a [PathBuf],
}

impl TraversalRules<'_> {
    fn should_visit(&self, entry: &DirEntry, package_root: &Path) -> bool {
        self.should_visit_path(entry.path(), package_root)
    }

    fn should_visit_path(&self, path: &Path, package_root: &Path) -> bool {
        if path.starts_with(self.target_directory) || path.file_name() == Some(OsStr::new(".git")) {
            return false;
        }

        path == package_root
            || !self
                .workspace_package_roots
                .iter()
                .any(|other_root| path == other_root)
    }
}

fn is_rust_file(entry: &DirEntry) -> bool {
    entry.file_type().is_file()
        && entry
            .path()
            .extension()
            .is_some_and(|extension| extension == OsStr::new("rs"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excludes_target_and_git_directories() {
        let package_root = Path::new("/workspace/crate");
        let target = Path::new("/workspace/target");
        let roots = vec![package_root.to_path_buf()];
        let rules = TraversalRules {
            target_directory: target,
            workspace_package_roots: &roots,
        };

        assert!(!rules.should_visit_path(target, package_root));
        assert!(!rules.should_visit_path(Path::new("/workspace/crate/.git"), package_root,));
    }

    #[test]
    fn does_not_descend_into_another_workspace_package() {
        let package_root = Path::new("/workspace");
        let child_root = PathBuf::from("/workspace/crates/child");
        let roots = vec![package_root.to_path_buf(), child_root.clone()];
        let rules = TraversalRules {
            target_directory: Path::new("/workspace/target"),
            workspace_package_roots: &roots,
        };

        assert!(rules.should_visit_path(package_root, package_root));
        assert!(!rules.should_visit_path(&child_root, package_root));
    }
}
