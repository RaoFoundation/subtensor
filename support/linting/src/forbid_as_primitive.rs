//! Ban panic-prone `as_u32` / `as_u64` / `as_u128` / `as_usize` conversions; prefer `try_into()`.

use super::*;
use syn::{ExprMethodCall, File, Ident, visit::Visit};

/// Lint: reject `as_u32`/`as_u64`/`as_u128`/`as_usize` method calls that can panic on overflow.
pub struct ForbidAsPrimitiveConversion;

impl Lint for ForbidAsPrimitiveConversion {
    fn lint(source: &File) -> Result {
        let mut visitor = AsPrimitiveVisitor::default();

        visitor.visit_file(source);

        if !visitor.errors.is_empty() {
            return Err(visitor.errors);
        }

        Ok(())
    }
}

#[derive(Default)]
struct AsPrimitiveVisitor {
    errors: Vec<syn::Error>,
}

impl<'ast> Visit<'ast> for AsPrimitiveVisitor {
    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        if is_banned_as_primitive_method(&node.method) {
            self.errors.push(syn::Error::new(
                node.method.span(),
                "Using 'as_*()' methods is banned to avoid accidental panics. Use `try_into()` instead.",
            ));
        }

        syn::visit::visit_expr_method_call(self, node);
    }
}

/// True for the panic-prone `as_u{32,64,128}` / `as_usize` conversion methods this lint bans.
fn is_banned_as_primitive_method(ident: &Ident) -> bool {
    matches!(
        ident.to_string().as_str(),
        "as_u32" | "as_u64" | "as_u128" | "as_usize"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    fn lint(input: proc_macro2::TokenStream) -> Result {
        let mut visitor = AsPrimitiveVisitor::default();
        #[allow(clippy::expect_used)]
        let expr: ExprMethodCall = syn::parse2(input).expect("should be a valid method call");
        visitor.visit_expr_method_call(&expr);
        if !visitor.errors.is_empty() {
            return Err(visitor.errors);
        }
        Ok(())
    }

    #[test]
    fn test_as_primitives() {
        let input = quote! {x.as_u32() };
        assert!(lint(input).is_err());
        let input = quote! {x.as_u64() };
        assert!(lint(input).is_err());
        let input = quote! {x.as_u128() };
        assert!(lint(input).is_err());
        let input = quote! {x.as_usize() };
        assert!(lint(input).is_err());
    }

    #[test]
    fn test_non_as_primitives() {
        let input = quote! {x.as_ref() };
        assert!(lint(input).is_ok());
        let input = quote! {x.as_slice() };
        assert!(lint(input).is_ok());
    }
}
