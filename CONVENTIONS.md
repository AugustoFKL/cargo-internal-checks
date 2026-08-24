# Rust conventions

This document defines the Rust source-layout conventions enforced by `cargo-internal-checks`. The presentation is
inspired by Sentry's [Rust Development](https://develop.sentry.dev/engineering-practices/rust/) guide, but the rules
below describe this tool's behavior rather than Sentry's policy.

The words **must**, **must not**, and **exactly** are normative. Anything not explicitly described as enforced is left
to `rustfmt`, another lint, or code review.

## Enforcement at a glance

| Convention                                          | Checked  | Fixed with `--fix` | Configurable |
|-----------------------------------------------------|:--------:|:------------------:|:------------:|
| Configured `use` and `mod` item order               |   Yes    |   Yes, when safe   |     Yes      |
| Standard-library, external, and local import order  |   Yes    |   Yes, when safe   |      No      |
| Blank lines between item classes and import origins |   Yes    |   Yes, when safe   |      No      |
| Alphabetical order inside one import group          |    No    | No; use `rustfmt`  |      No      |
| Alphabetical error variants                         |   Yes    |   Yes, when safe   |      No      |
| Exactly one empty line between error variants       |   Yes    |   Yes, when safe   |      No      |
| Ordinary comments between error variants            | Rejected |         No         |      No      |

The checker is authoritative. `--fix` is deliberately conservative: a file can violate a convention even when the tool
refuses to rewrite the ambiguous source automatically.

## Module item order

### Rule

Every contiguous run of configured `use` and `mod` items must follow the class order declared in
`internal-checks.toml`. With the repository's default configuration, that order is:

1. `use`
2. `pub(crate) use`
3. `pub use`
4. `mod`
5. `pub(crate) mod`
6. `pub mod`

Visibility is part of the class. For example, every `pub(crate) use` in a run must appear after all private `use` items
and before all `pub use` items.

### Why

A stable declaration order makes visibility changes obvious and gives each module the same high-level shape. The order
is repository-configurable because declaration layout is policy, not a language requirement.

### Scope and boundaries

A run continues only across item classes included in `order`. Any other Rust item ends the run, including a function,
type, constant, or macro invocation. A supported class omitted from the configuration also ends the run instead of being
silently assigned a rank.

The only supported visibilities are private, `pub(crate)`, and `pub`. Restricted forms such as `pub(super)` and
`pub(in path)` are not ordered and therefore end a run.

Outer attributes and ordinary comments do not end a checker run because the checker operates on parsed items. Inline
modules are checked recursively and independently, using the declarations in each module as that module's local scope.

### Examples

This run is invalid because `pub(crate) use` is configured before `pub use`:

```rust
pub use crate::Public;

pub(crate) use crate::Internal;
```

This is valid because the constant ends the first run:

```rust
pub use crate::Public;

const CAPACITY: usize = 16;

use crate::Private;
```

### Automatic fixing

`--fix` stably sorts a run by configured class and import origin. It moves outer attributes with their item and
preserves the existing order inside one import group.

The fixer leaves an entire run unchanged when non-whitespace text occurs between its items. This includes ordinary
comments, whose ownership cannot be inferred safely. The checker can still report ordering or spacing violations in that
run.

## Import origin groups

### Rule

Within each `use` visibility class, imports must appear in this order:

1. standard library;
2. external dependencies;
3. local modules.

Adjacent imports from different origins must have at least one empty line between them. A blank line is also required
whenever adjacent configured items have different visibility classes or when the sequence changes between an import
class and a module class.

`pub use` is a visibility class, not an origin. For example, `pub use anyhow::Error` is an external import while
`pub use crate::Error` is a local import; both remain in the position assigned to `pub use` by the configuration.

### Origin classification

| Origin           | Path roots                                                                   |
|------------------|------------------------------------------------------------------------------|
| Standard library | `std`, `core`, and `alloc`                                                   |
| Local            | `self`, `super`, `crate`, or the name of a `mod` declared in the same module |
| External         | Every other root, including workspace dependencies                           |

A local module declaration takes precedence over the reserved standard-library roots. Consequently, `use core::Value`
is local in a module that declares `mod core;`; otherwise it is a standard-library import.

### Why

Origin groups show dependencies at a glance and keep imports stable without duplicating `rustfmt`'s responsibility for
formatting and ordering paths within one group.

### Example

```rust
use std::path::Path;

use anyhow::Result;
use tracing::info;

use crate::config::Config;
use super::Context;

pub use anyhow::Error;

mod parser;
```

The checker does **not** require `anyhow` to precede `tracing`; alphabetical ordering within the external group is left
to the project's `rustfmt` configuration. It only enforces the group order and the blank lines at group boundaries.

### Automatic fixing

`--fix` groups imports by origin and inserts the required blank lines. It does not change import granularity,
alphabetize paths within a group, or invoke `rustfmt`. Run the repository's normal formatting command afterward.

## Error variants

### Recognition and scope

The rule applies to any enum with a `derive` path whose final segment is named `Error`. This includes `Error`,
`thiserror::Error`, and other qualified derives ending in `::Error`. Enums without such a derive are ignored. Error
enums inside inline modules are checked recursively.

### Rule

Error variants must be ordered alphabetically by identifier. Comparison is case-sensitive and uses the identifier
without a raw-identifier prefix, so `r#match` is ordered as `match`.

Exactly one empty line must separate consecutive variant blocks. Attributes and Rustdoc belong to the variant that
follows them, so the empty line appears before that variant's first attribute or Rustdoc line.

```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The input could not be decoded.
    #[error("invalid input")]
    InvalidInput,

    #[error("request timed out")]
    Timeout,
}
```

The checker evaluates order and spacing independently. One pair of variants can therefore produce both diagnostics.

### Comments between variants

Ordinary line comments, trailing comments, and block comments must not appear between variants. Their ownership becomes
ambiguous when variants are reordered:

```rust
#[derive(Error)]
enum Error {
    First,

    // Does this document First or Second?
    Second,
}
```

Use Rustdoc when text documents the following variant:

```rust
#[derive(Error)]
enum Error {
    First,

    /// Explains Second.
    Second,
}
```

### Why

Alphabetical order makes an error enum predictable to scan and reduces arbitrary placement decisions. Exact spacing
keeps attributes, Rustdoc, and variant bodies visually distinct. Rejecting ordinary inter-variant comments prevents an
automatic reorder from silently changing what a comment appears to describe.

### Automatic fixing

`--fix` alphabetizes variants and normalizes their spacing while preserving attributes, Rustdoc, variant bodies, newline
style, and whether the final variant has a trailing comma.

The fixer leaves the enum unchanged when it finds ambiguous text between variants, nonstandard punctuation, or
non-whitespace text immediately inside the opening or closing brace. This includes ordinary comments. Detection remains
stricter than automatic fixing by design.
