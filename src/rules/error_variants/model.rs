use syn::{ItemEnum, Path, Token, Variant, punctuated::Punctuated};

#[derive(Debug, Clone, Copy)]
pub(crate) struct ErrorEnum<'a> {
    ast: &'a ItemEnum,
}

impl<'a> ErrorEnum<'a> {
    pub(crate) fn from_ast(ast: &'a ItemEnum) -> Option<Self> {
        for attribute in &ast.attrs {
            if !attribute.path().is_ident("derive") {
                continue;
            }

            let Ok(paths) =
                attribute.parse_args_with(Punctuated::<Path, Token![,]>::parse_terminated)
            else {
                continue;
            };

            for path in paths {
                if Self::path_ends_in_error(&path) {
                    return Some(Self { ast });
                }
            }
        }

        None
    }

    pub(crate) fn ast(self) -> &'a ItemEnum {
        self.ast
    }

    pub(crate) fn variants(self) -> &'a Punctuated<Variant, Token![,]> {
        &self.ast.variants
    }

    fn path_ends_in_error(path: &Path) -> bool {
        let Some(segment) = path.segments.last() else {
            return false;
        };

        segment.ident == "Error"
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Result;

    use super::*;

    fn enum_with_derive(derive: &str) -> Result<ItemEnum> {
        Ok(syn::parse_str(&format!(
            "#[derive({derive})] enum Example {{}}"
        ))?)
    }

    #[test]
    fn recognizes_unqualified_and_qualified_error_derives() -> Result<()> {
        assert!(ErrorEnum::from_ast(&enum_with_derive("Error")?).is_some());
        assert!(ErrorEnum::from_ast(&enum_with_derive("Debug, thiserror::Error")?).is_some());
        Ok(())
    }

    #[test]
    fn rejects_other_derives() -> Result<()> {
        assert!(ErrorEnum::from_ast(&enum_with_derive("Debug, Clone")?).is_none());
        let malformed = syn::parse_str(
            r#"#[allow(dead_code)]
            #[derive(Debug = "invalid")]
            enum Example {}"#,
        )?;
        assert!(ErrorEnum::from_ast(&malformed).is_none());
        Ok(())
    }
}
