# cargo-internal-checks

`cargo-internal-checks` checks and fixes the ordering of selected Rust module items according to a repository-local
configuration. It parses Rust structurally with `syn`, checks inline modules recursively, discovers workspace packages
through Cargo metadata, and exits non-zero when it finds an ordering violation.

## Configuration

Create `internal-checks.toml` at the Cargo workspace root:

```toml
order = [
    "use",
    "pub(crate) use",
    "pub use",
    "mod",
    "pub(crate) mod",
    "pub mod",
]
```

The supported item classes in v0.1 are:

- `use`
- `pub(crate) use`
- `pub use`
- `mod`
- `pub(crate) mod`
- `pub mod`

Classes omitted from `order` are unconstrained and act as boundaries between ordered runs.

Other restricted visibilities such as `pub(super)` and `pub(in path)` are also unconstrained boundaries in v0.1.

## Conventions

[Rust conventions](CONVENTIONS.md) is the normative reference for every rule enforced by this tool. Anything not
identified there as enforced remains the responsibility of `rustfmt`, another lint, or code review.

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

Use a different manifest or configuration file with:

```bash
cargo internal-checks --manifest-path path/to/Cargo.toml
cargo internal-checks --config path/to/internal-checks.toml
```

The default configuration path is `<workspace-root>/internal-checks.toml`.

Diagnostics use workspace-relative paths by default. Pass `-v` or `--verbose` to display absolute paths instead:

```bash
cargo internal-checks --verbose
```

The checker returns:

- exit code `0` when no violations are found;
- exit code `1` when ordering violations are found;
- exit code `2` for configuration, parsing, discovery, or I/O failures.

That makes the default command suitable for CI without an additional `--check` flag.

### Applying fixes

Pass `--fix` to reorder consecutive imports and module declarations, and to order and space variants in enums deriving
`Error`. Items follow the configured visibility order, while imports are additionally grouped as standard-library,
external, and local imports:

```bash
cargo internal-checks --fix
```

`--fix` does not invoke rustfmt; run the project’s normal formatting command afterward.

## File discovery

For each selected package, the checker recursively scans Rust files under the package directory. It does not follow
symlinks and does not descend into Cargo's target directory or `.git`. Results are deduplicated, which also avoids
duplicate checks when package roots overlap.

## Releasing

Releases are built by GitHub Actions when a tag matching `v*` is pushed. The tag must match the package version in
`Cargo.toml`; for example, version `0.1.0` must be released with tag `v0.1.0`.

```bash
git tag v0.1.0
git push origin v0.1.0
```

The workflow publishes archives for Windows x86-64, macOS Intel, and macOS Apple Silicon. Each release also includes a
`SHA256SUMS` file.
