# cargo-internal-checks

`cargo-internal-checks` checks and fixes the ordering of selected Rust module items according to a repository-local configuration. It parses Rust structurally with `syn`, checks inline modules recursively, discovers workspace packages through Cargo metadata, and exits non-zero when it finds an ordering violation.

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

## Ordering semantics

Ordering is checked independently within each contiguous run of configured items.

This is invalid:

```rust
pub use crate::a::A;
pub(crate) use crate::b::B;
```

This is valid because the constant ends the first ordered run:

```rust
pub use crate::a::A;

const VALUE: usize = 3;

use crate::b::B;
```

Inline modules are checked recursively:

```rust
mod outer {
    mod inner {
        pub mod public;
        use crate::a::A; // violation
    }
}
```

Outer attributes and ordinary comments do not create checker run boundaries because the checker operates on parsed Rust items rather than lines. The fixer moves outer attributes together with their item and conservatively leaves runs containing comments unchanged.

Within each import visibility class, imports are grouped as standard-library, external, and local imports. Ordering inside one of those groups is left to `rustfmt`, which avoids duplicating formatter policy in this tool. Bare roots matching a module declared in the same scope are local imports. This local declaration takes precedence when its name is also a standard-library root, so `use core::*` is local in a scope containing `mod core;` and otherwise refers to Rust's `core` crate for grouping purposes.

### Error variants

Enums deriving `Error` or `thiserror::Error` must declare their variants alphabetically by identifier. Variant blocks must be separated by exactly one empty line; attributes and Rustdoc remain attached to the variant that follows them.

Ordinary comments between error variants are rejected with a dedicated diagnostic because their ownership is ambiguous when variants are reordered. Use Rustdoc (`///`) when a comment describes the following variant.

```rust
#[derive(Debug, Error)]
pub enum Error {
    #[error("message 1")]
    Variant1 {},

    #[error("message 2")]
    Variant2,
}
```

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

The checker returns:

- exit code `0` when no violations are found;
- exit code `1` when ordering violations are found;
- exit code `2` for configuration, parsing, discovery, or I/O failures.

That makes the default command suitable for CI without an additional `--check` flag.

### Fixing item order

Pass `--fix` to reorder consecutive imports and module declarations, and to order and space variants in enums deriving `Error`. Items follow the configured visibility order, while imports are additionally grouped as standard-library, external, and local imports:

```bash
cargo internal-checks --fix
```

The fixer orders the groups and inserts the required blank lines, including the boundary between imports and modules, boundaries between module visibility classes, and exactly one empty line between error variants. Attributes, Rustdoc, variant bodies, line endings, and the enum's trailing-comma policy are preserved. It does not invoke `rustfmt`, because projects may pin a particular toolchain or require nightly formatting options. After fixing, run the project's normal formatting command to handle ordering and granularity within each import group.

Comments between imports or error variants make their ownership ambiguous, so the fixer leaves those runs unchanged and the normal check reports any remaining violations. The same rule applies to comments immediately inside an error enum's braces, where sorting could otherwise attach them to a different variant.

## File discovery

For each selected package, the checker recursively scans Rust files under the package directory. It does not follow symlinks and does not descend into Cargo's target directory or `.git`. Results are deduplicated, which also avoids duplicate checks when package roots overlap.

## Releasing

Releases are built by GitHub Actions when a tag matching `v*` is pushed. The tag must match the package version in `Cargo.toml`; for example, version `0.1.0` must be released with tag `v0.1.0`.

```bash
git tag v0.1.0
git push origin v0.1.0
```

The workflow publishes archives for Windows x86-64, macOS Intel, and macOS Apple Silicon. Each release also includes a `SHA256SUMS` file.
