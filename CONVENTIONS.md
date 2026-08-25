# Rust conventions

This document defines the Rust source-layout conventions enforced by `cargo-internal-checks`. The presentation is
inspired by Sentry's [Rust Development](https://develop.sentry.dev/engineering-practices/rust/) guide, but the rules
below describe this tool's behavior rather than Sentry's policy.

The words **must**, **must not**, and **exactly** are normative. Anything not explicitly described as enforced is left
to `rustfmt`, another lint, or code review.

## Enforcement at a glance

| Convention                                               | Checked  | Fixed with `--fix` |
|----------------------------------------------------------|:--------:|:------------------:|
| `use` and `mod` item order                               |   Yes    |   Yes, when safe   |
| Conventional `#[cfg(test)] mod tests` placement/privacy  |   Yes    |   Placement only   |
| Standard-library, external, and local import order       |   Yes    |   Yes, when safe   |
| Blank lines between item classes and import origins      |   Yes    |   Yes, when safe   |
| No blank lines within one visibility/origin import group |   Yes    |   Yes, when safe   |
| Alphabetical order inside one import group               |    No    | No; use `rustfmt`  |
| Ordinary comments inside an ordered `use`/`mod` run      | Rejected |         No         |
| Alphabetical error variants                              |   Yes    |   Yes, when safe   |
| Exactly one empty line between error variants            |   Yes    |   Yes, when safe   |
| Ordinary comments between error variants                 | Rejected |         No         |

The checker is authoritative. `--fix` is deliberately conservative: a file can violate a convention even when the tool
refuses to rewrite the ambiguous source automatically.

## Module item order

### Rule

Every contiguous run of supported `use` and `mod` items must follow this order:

1. `use`
2. `pub(crate) use`
3. `pub use`
4. `mod`
5. `pub(crate) mod`
6. `pub mod`
7. private `#[cfg(test)] mod tests`

Visibility is part of the class. For example, every `pub(crate) use` in a run must appear after all private `use` items
and before all `pub use` items.

The final class is recognized only when a module is named `tests` and has a direct `#[cfg(test)]` attribute. It must use
private visibility. A differently named `#[cfg(test)]` module remains an ordinary module, so test-only support modules
can retain their normal place in the module order. Compound predicates such as `#[cfg(all(test, feature = "extra"))]`
are not treated as the conventional test module.

### Why

A stable declaration order makes visibility changes obvious and gives each module the same high-level shape. Keeping
the order in the tool ensures that the team's repositories share one convention.

### Scope and boundaries

A run continues only across supported `use` and `mod` items. Any other Rust item ends the run, including a function,
type, constant, or macro invocation.

The only supported visibilities are private, `pub(crate)`, and `pub`. Restricted forms such as `pub(super)` and
`pub(in path)` are not ordered and therefore end a run.

The conventional test module is required to be last only within its contiguous run. This rule does not move it across
functions, types, macros, or other items that end a run.

Outer attributes do not end a checker run and move with their item. Inline modules are checked recursively and
independently, using the declarations in each module as that module's local scope.

### Comments within runs

Ordinary line and block comments must not appear between items in an ordered `use`/`mod` run. Their ownership becomes
ambiguous when the run is reordered, so they prevent `--fix` from changing anything in that run. While such a comment
remains, the checker reports the comment instead of downstream order or spacing violations from the blocked run.

Use Rustdoc (`///`) when a comment documents the following item. Otherwise, move the comment outside the ordered run,
such as after the import and module declarations.

### Examples

This run is invalid because `pub(crate) use` must appear before `pub use`:

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

This is valid because `test_support` remains an ordinary private module while the conventional `tests` module ends the
run:

```rust
#[cfg(test)]
mod test_support {}

pub mod code_module {}

#[cfg(test)]
mod tests {}
```

Writing `pub mod tests` or any other explicit visibility on the conventional test module is invalid.

### Automatic fixing

`--fix` stably sorts a run by class and import origin. It moves outer attributes with their item, moves the conventional
test module to the end of its run, and preserves the existing order inside one import group. It does not remove an
invalid visibility from the test module; that diagnostic requires a manual change.

The fixer leaves an entire run unchanged when non-whitespace text occurs between its items. For ordinary comments, the
checker reports the actionable comment diagnostic described above.

## Import origin groups

### Rule

Within each `use` visibility class, imports must appear in this order:

1. standard library;
2. external dependencies;
3. local modules.

Adjacent imports from different origins must have at least one empty line between them. A blank line is also required
whenever adjacent supported items have different visibility classes or when the sequence changes between an import
class and a module class.

Conversely, consecutive imports in the same visibility class and origin group must not have an empty line between them.
They remain separate `use` items; this convention controls only their separator.

`pub use` is a visibility class, not an origin. For example, `pub use anyhow::Error` is an external import while
`pub use crate::Error` is a local import; both remain in the position assigned to `pub use` by the fixed class order.

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

use crate::diagnostic::Violation;
use super::Context;

pub use anyhow::Error;

mod parser;
```

The checker does **not** require `anyhow` to precede `tracing`; alphabetical ordering within the external group is left
to the project's `rustfmt` configuration. It only enforces the group order and the blank lines at group boundaries.

This is invalid because both imports are private and local, so the empty line splits one import group:

```rust
use crate::encoding::Decode;

use crate::schemes::NgfheScheme;
```

Remove only the empty line; the two `use` trees do not need to be merged:

```rust
use crate::encoding::Decode;
use crate::schemes::NgfheScheme;
```

### Automatic fixing

`--fix` groups imports by origin, inserts required blank lines, and removes blank lines inside one group. It does not
change import granularity, alphabetize paths within a group, or invoke `rustfmt`. Run the repository's normal formatting
command afterward.

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

The checker evaluates order and spacing independently. One pair of variants can therefore produce both diagnostics,
unless an ordinary comment blocks the enum from being fixed safely.

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

When an enum contains ordinary comments between variants, the checker reports those comments and suppresses its
alphabetical-order and spacing diagnostics for that enum. Resolve every comment first; the next `--fix` run can then
order and space the variants safely.

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
