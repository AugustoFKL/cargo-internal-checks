# cargo-item-order

`cargo-item-order` checks the ordering of selected Rust module items according to a repository-local configuration.

The first version is intentionally check-only. It parses Rust structurally with `syn`, checks inline modules recursively, discovers workspace packages through Cargo metadata, and exits non-zero when it finds an ordering violation.

## Configuration

Create `item-order.toml` at the Cargo workspace root:

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

Attributes and comments do not create run boundaries because the checker operates on parsed Rust items rather than lines.

## Usage

Install the binary so `cargo-item-order` is on `PATH`, then invoke it as a Cargo subcommand:

```bash
cargo item-order
```

By default, every workspace package is checked. Select one or more packages with:

```bash
cargo item-order -p math
cargo item-order -p math -p crypto
```

Use a different manifest or configuration file with:

```bash
cargo item-order --manifest-path path/to/Cargo.toml
cargo item-order --config path/to/item-order.toml
```

The default configuration path is `<workspace-root>/item-order.toml`.

The checker returns:

- exit code `0` when no violations are found;
- exit code `1` when ordering violations are found;
- exit code `2` for configuration, parsing, discovery, or I/O failures.

That makes the default command suitable for CI without an additional `--check` flag.

## File discovery

For each selected package, the checker recursively scans Rust files under the package directory. It does not follow symlinks and does not descend into Cargo's target directory or `.git`. Results are deduplicated, which also avoids duplicate checks when package roots overlap.

## Releasing

Releases are built by GitHub Actions when a tag matching `v*` is pushed. The tag must match the package version in `Cargo.toml`; for example, version `0.1.0` must be released with tag `v0.1.0`.

```bash
git tag v0.1.0
git push origin v0.1.0
```

The workflow publishes archives for Windows x86-64, macOS Intel, and macOS Apple Silicon. Each release also includes a `SHA256SUMS` file.

## Planned next step

The natural v0.2 feature is `--fix`. The analyzer is deliberately separate from file discovery so the fixer can reuse the same classifications and violations while applying source-preserving textual edits instead of reprinting the `syn` AST.
