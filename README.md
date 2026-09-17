# cargo-internal-checks

`cargo-internal-checks` checks and fixes the team's Rust source-layout conventions. It parses Rust structurally with
`syn`, checks inline modules recursively, discovers workspace packages through Cargo metadata, and exits non-zero when
it finds a violation.

## Conventions

[Rust conventions](CONVENTIONS.md) is the normative reference for every rule enforced by this tool. Anything not
identified there as enforced remains the responsibility of `rustfmt`, another lint, or code review.

The conventions are built into the binary so every checked repository follows the same policy. No configuration file
is required.

## Usage

Install the binary so `cargo-internal-checks` is on `PATH`, then invoke it as a Cargo subcommand:

```bash
cargo internal-checks
```

By default, every workspace package is checked. Select one or more packages with:

```bash
cargo internal-checks -p math
cargo internal-checks -p math -p crypto
```

Use a different manifest with:

```bash
cargo internal-checks --manifest-path path/to/Cargo.toml
```

### Selecting files and directories

Use `--path` to limit checking to particular Rust files or directories:

```bash
cargo internal-checks --path src/schemes/ngfhe/error.rs
cargo internal-checks --path src/schemes/ngfhe
cargo internal-checks --path crates/math --path crates/crypto
```

A file selects exactly that Rust source file. A directory selects Rust files recursively beneath it, and `--path` may
be repeated. Relative paths are resolved from the directory where the command is invoked.

Path selection is applied within the packages selected by `-p`, or within all workspace packages when `-p` is omitted.
Missing paths, non-Rust files, and paths that contain no Rust files from the selected packages are errors. `--fix`
modifies only the selected files.

Paths select filesystem files rather than Rust modules. All inline modules within a selected file are checked; an
individual inline module cannot be selected independently.

### Selecting changed files

Use `--changed` to check only Rust files changed in the current Git worktree or index:

```bash
cargo internal-checks --changed
cargo internal-checks --fix --changed
cargo internal-checks -p math --path src --changed
```

Changed files include staged changes, unstaged changes, renames at their current paths, and non-ignored untracked files.
Deleted files are ignored. `--changed` intersects with `-p` and `--path`; when no eligible Rust files have changed, the
command succeeds after checking zero files. The command reports an operational error when the selected workspace is not
inside a Git repository or Git cannot determine its status.

Selection is file-based rather than line-based. `--fix --changed` may therefore rewrite an entire import run or error
enum within a changed file. When a file is partially staged, fixing it can create additional unstaged changes without
changing the version already in Git's index.

`--changed` describes local index and worktree changes relative to `HEAD`. It does not include files that are already
committed on the current branch but differ from another branch such as `main`.

Use `--changed-since REV` to include those committed branch changes as well as current local changes:

```bash
cargo internal-checks --changed-since origin/main
cargo internal-checks --fix --changed-since main
cargo internal-checks -p math --path src --changed-since HEAD~3
```

`REV` must resolve to a commit, such as a branch, tag, commit hash, or expression like `HEAD~3`. The command selects the
union of files changed from the merge base of `REV` and `HEAD` to `HEAD`, plus the staged, unstaged, renamed, and
non-ignored untracked files selected by `--changed`. This isolates the current branch's changes when `REV` is another
branch. `--changed` and `--changed-since` are mutually exclusive because `--changed-since` already includes local
changes.

The comparison requires enough repository history to find a merge base. An invalid revision, unrelated history, or an
insufficiently deep clone produces an operational error. Fixing still modifies only current working-tree files;
deleted files are ignored.

Diagnostics use workspace-relative paths by default. Pass `-v` or `--verbose` to display absolute paths instead:

```bash
cargo internal-checks --verbose
```

The checker returns:

- exit code `0` when no violations are found;
- exit code `1` when ordering violations are found;
- exit code `2` for parsing, discovery, or I/O failures.

That makes the default command suitable for CI without an additional `--check` flag.

### Applying fixes

Pass `--fix` to reorder consecutive imports and module declarations, and to order and space variants in enums deriving
`Error`. Items follow the team's visibility order, while imports are additionally grouped as standard-library,
external, and local imports:

```bash
cargo internal-checks --fix
```

`--fix` does not invoke rustfmt; run the project’s normal formatting command afterward.

## File discovery

For each selected package, the checker recursively scans Rust files under the package directory, then applies any
`--path` and Git-change filters. It does not follow symlinks and does not descend into Cargo's target directory or
`.git`. Results are deduplicated, which also avoids duplicate checks when package roots overlap or selected paths
overlap.

## Releasing

Releases are built by GitHub Actions when a tag matching `v*` is pushed. The tag must match the package version in
`Cargo.toml`; for example, version `0.1.0` must be released with tag `v0.1.0`.

```bash
git tag v0.1.0
git push origin v0.1.0
```

The workflow publishes archives for Windows x86-64, macOS Intel, and macOS Apple Silicon. Each release also includes a
`SHA256SUMS` file.
