//! Require `#[freeze_struct("…")]` on every struct that derives `Encode` and/or `Decode`.
//!
//! Layout hashes catch accidental SCALE field reorder/type changes that corrupt storage.

use super::*;
use syn::{
    Attribute, File, ItemStruct, Meta, MetaList, Path, Token, parse_quote, punctuated::Punctuated,
    visit::Visit,
};

/// Lint: structs deriving `Encode`/`Decode` must also carry `#[freeze_struct("hash")]`.
pub struct RequireFreezeStruct;

impl Lint for RequireFreezeStruct {
    fn lint(source: &File) -> Result {
        let mut visitor = FreezeStructEncodeDecodeVisitor::default();

        visitor.visit_file(source);

        if !visitor.errors.is_empty() {
            return Err(visitor.errors);
        }

        Ok(())
    }
}

#[derive(Default)]
struct FreezeStructEncodeDecodeVisitor {
    errors: Vec<syn::Error>,
}

impl<'ast> Visit<'ast> for FreezeStructEncodeDecodeVisitor {
    fn visit_item_struct(&mut self, node: &'ast ItemStruct) {
        let has_encode_decode = node.attrs.iter().any(is_derive_encode_or_decode);
        let has_freeze_struct = node.attrs.iter().any(attr_is_freeze_struct);

        if has_encode_decode && !has_freeze_struct {
            self.errors.push(syn::Error::new(
                node.ident.span(),
                "Struct with Encode/Decode derive must also have #[freeze_struct(..)] attribute.",
            ));
        }

        syn::visit::visit_item_struct(self, node);
    }
}

/// True when the attribute is `#[freeze_struct("…")]` with a non-empty hash argument.
fn attr_is_freeze_struct(attr: &Attribute) -> bool {
    if let Meta::List(meta_list) = &attr.meta {
        let Some(seg) = meta_list.path.segments.last() else {
            return false;
        };
        if seg.ident == "freeze_struct" && !meta_list.tokens.is_empty() {
            return true;
        }
    }
    false
}

fn is_derive_encode_or_decode(attr: &Attribute) -> bool {
    if let Meta::List(MetaList { path, tokens, .. }) = &attr.meta
        && path.is_ident("derive")
    {
        let nested: Punctuated<Path, Token![,]> = parse_quote!(#tokens);
        return nested.iter().any(|nested| {
            nested.segments.iter().any(|seg| seg.ident == "Encode")
                || nested.segments.iter().any(|seg| seg.ident == "Decode")
        });
    }
    false
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;

    fn lint_struct(input: &str) -> Result {
        let item_struct: ItemStruct = syn::parse_str(input).expect("should only use on a struct");
        let mut visitor = FreezeStructEncodeDecodeVisitor::default();
        visitor.visit_item_struct(&item_struct);
        if !visitor.errors.is_empty() {
            return Err(visitor.errors);
        }
        Ok(())
    }

    #[test]
    fn test_no_attributes() {
        let input = r#"
            pub struct Test {
                field: u32,
            }
        "#;
        assert!(lint_struct(input).is_ok());
    }

    #[test]
    fn test_freeze_struct_only() {
        let input = r#"
            #[freeze_struct("12345")]
            pub struct Test {
                field: u32,
            }
        "#;
        assert!(lint_struct(input).is_ok());
    }

    #[test]
    fn test_encode_only() {
        let input = r#"
            #[derive(Encode)]
            pub struct Test {
                field: u32,
            }
        "#;
        assert!(lint_struct(input).is_err());
    }

    #[test]
    fn test_decode_only() {
        let input = r#"
            #[derive(Decode)]
            pub struct Test {
                field: u32,
            }
        "#;
        assert!(lint_struct(input).is_err());
    }

    #[test]
    fn test_encode_and_freeze_struct() {
        let input = r#"
            #[freeze_struct("12345")]
            #[derive(Encode)]
            pub struct Test {
                field: u32,
            }
        "#;
        assert!(lint_struct(input).is_ok());
    }

    #[test]
    fn test_decode_and_freeze_struct() {
        let input = r#"
            #[freeze_struct("12345")]
            #[derive(Decode)]
            pub struct Test {
                field: u32,
            }
        "#;
        assert!(lint_struct(input).is_ok());
    }

    #[test]
    fn test_encode_decode_without_freeze_struct() {
        let input = r#"
            #[derive(Encode, Decode)]
            pub struct Test {
                field: u32,
            }
        "#;
        assert!(lint_struct(input).is_err());
    }

    #[test]
    fn test_encode_decode_with_freeze_struct() {
        let input = r#"
            #[freeze_struct("12345")]
            #[derive(Encode, Decode)]
            pub struct Test {
                field: u32,
            }
        "#;
        assert!(lint_struct(input).is_ok());
    }

    #[test]
    fn test_temporary_freeze_struct() {
        let input = r#"
            #[freeze_struct]
            #[derive(Encode, Decode)]
            pub struct Test {
                field: u32,
            }
        "#;
        assert!(lint_struct(input).is_err());
    }
}
