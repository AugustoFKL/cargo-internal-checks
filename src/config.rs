use std::{
    collections::{HashMap, HashSet},
    fmt, fs,
    path::Path,
};

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ItemKind {
    Use,
    Mod,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Visibility {
    Private,
    Crate,
    Public,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ItemClass {
    kind: ItemKind,
    visibility: Visibility,
}

impl ItemClass {
    pub(crate) const fn new(kind: ItemKind, visibility: Visibility) -> Self {
        Self { kind, visibility }
    }

    fn parse(value: &str) -> Option<Self> {
        let class = match value {
            "use" => Self::new(ItemKind::Use, Visibility::Private),
            "pub(crate) use" => Self::new(ItemKind::Use, Visibility::Crate),
            "pub use" => Self::new(ItemKind::Use, Visibility::Public),
            "mod" => Self::new(ItemKind::Mod, Visibility::Private),
            "pub(crate) mod" => Self::new(ItemKind::Mod, Visibility::Crate),
            "pub mod" => Self::new(ItemKind::Mod, Visibility::Public),
            _ => return None,
        };
        Some(class)
    }
}

impl fmt::Display for ItemClass {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match (self.kind, self.visibility) {
            (ItemKind::Use, Visibility::Private) => "use",
            (ItemKind::Use, Visibility::Crate) => "pub(crate) use",
            (ItemKind::Use, Visibility::Public) => "pub use",
            (ItemKind::Mod, Visibility::Private) => "mod",
            (ItemKind::Mod, Visibility::Crate) => "pub(crate) mod",
            (ItemKind::Mod, Visibility::Public) => "pub mod",
        };
        formatter.write_str(text)
    }
}

#[derive(Debug)]
pub(crate) struct Config {
    ranks: HashMap<ItemClass, usize>,
}

impl Config {
    pub(crate) fn load(path: &Path) -> Result<Self> {
        let source = fs::read_to_string(path)
            .with_context(|| format!("failed to read config `{}`", path.display()))?;
        Self::parse(&source).with_context(|| format!("failed to parse config `{}`", path.display()))
    }

    pub(crate) fn parse(source: &str) -> Result<Self> {
        let raw: RawConfig = toml::from_str(source).context("invalid TOML")?;
        if raw.order.is_empty() {
            bail!("`order` must contain at least one item class");
        }

        let mut seen = HashSet::with_capacity(raw.order.len());
        let mut ranks = HashMap::with_capacity(raw.order.len());

        for (rank, value) in raw.order.iter().enumerate() {
            let class = ItemClass::parse(value).ok_or_else(|| {
                anyhow!(
                    "unsupported item class `{value}`; expected one of: use, pub(crate) use, pub use, mod, pub(crate) mod, pub mod"
                )
            })?;

            if !seen.insert(class) {
                bail!("duplicate item class `{value}` in `order`");
            }

            ranks.insert(class, rank);
        }

        Ok(Self { ranks })
    }

    pub(crate) fn rank(&self, class: ItemClass) -> Option<usize> {
        self.ranks.get(&class).copied()
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    order: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_configured_order() -> Result<()> {
        let config = Config::parse(
            r#"
            order = ["pub use", "use"]
            "#,
        )?;

        assert_eq!(
            config.rank(ItemClass::new(ItemKind::Use, Visibility::Public)),
            Some(0)
        );
        assert_eq!(
            config.rank(ItemClass::new(ItemKind::Use, Visibility::Private)),
            Some(1)
        );
        assert_eq!(
            config.rank(ItemClass::new(ItemKind::Mod, Visibility::Private)),
            None
        );
        Ok(())
    }

    #[test]
    fn formats_every_supported_item_class() {
        let cases = [
            (ItemKind::Use, Visibility::Private, "use"),
            (ItemKind::Use, Visibility::Crate, "pub(crate) use"),
            (ItemKind::Use, Visibility::Public, "pub use"),
            (ItemKind::Mod, Visibility::Private, "mod"),
            (ItemKind::Mod, Visibility::Crate, "pub(crate) mod"),
            (ItemKind::Mod, Visibility::Public, "pub mod"),
        ];

        for (kind, visibility, expected) in cases {
            assert_eq!(ItemClass::new(kind, visibility).to_string(), expected);
        }
    }

    #[test]
    fn rejects_invalid_configurations() -> Result<()> {
        let cases = [
            (r#"order = []"#, "must contain at least one item class"),
            (r#"order = ["use", "use"]"#, "duplicate item class"),
            (r#"order = ["pub(super) use"]"#, "unsupported item class"),
        ];

        for (source, expected) in cases {
            let error = Config::parse(source)
                .err()
                .ok_or(anyhow!("configuration should be rejected"))?;
            assert!(error.to_string().contains(expected));
        }

        Ok(())
    }
}
