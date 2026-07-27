//! Ban `saturating_*` arithmetic so overflowing tests panic instead of silently clamping.

use super::*;
use syn::{Expr, ExprCall, ExprMethodCall, ExprPath, File, Path, spanned::Spanned, visit::Visit};

/// Lint: reject `saturating_*` methods and UFCS calls (production code should use checked/overflowing ops or accept panics in tests).
pub struct ForbidSaturatingMath;

impl Lint for ForbidSaturatingMath {
    fn lint(source: &File) -> Result {
        let mut visitor = SaturatingMathBanVisitor::default();
        visitor.visit_file(source);

        if visitor.errors.is_empty() {
            Ok(())
        } else {
            Err(visitor.errors)
        }
    }
}

#[derive(Default)]
struct SaturatingMathBanVisitor {
    errors: Vec<syn::Error>,
}

impl<'ast> Visit<'ast> for SaturatingMathBanVisitor {
    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        let ExprMethodCall { method, .. } = node;

        if method.to_string().starts_with("saturating_") {
            let msg = "Safe math is banned to encourage tests to panic";
            self.errors.push(syn::Error::new(method.span(), msg));
        }
    }

    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        let ExprCall { func, .. } = node;

        if is_saturating_math_call(func) {
            let msg = "Safe math is banned to encourage tests to panic";
            self.errors.push(syn::Error::new(node.func.span(), msg));
        }
    }
}

fn is_saturating_math_call(func: &Expr) -> bool {
    let Expr::Path(ExprPath {
        path: Path { segments: path, .. },
        ..
    }) = func
    else {
        return false;
    };

    path.last()
        .is_some_and(|seg| seg.ident.to_string().starts_with("saturating_"))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;
    use quote::quote;

    fn lint(input: proc_macro2::TokenStream) -> Result {
        let mut visitor = SaturatingMathBanVisitor::default();
        let expr: syn::Expr = syn::parse2(input).expect("should be a valid expression");

        match &expr {
            syn::Expr::MethodCall(call) => visitor.visit_expr_method_call(call),
            syn::Expr::Call(call) => visitor.visit_expr_call(call),
            _ => panic!("should be a valid method call or function call"),
        }

        if visitor.errors.is_empty() {
            Ok(())
        } else {
            Err(visitor.errors)
        }
    }

    #[test]
    fn test_saturating_forbidden() {
        let input = quote! { stake.saturating_add(alpha) };
        assert!(lint(input).is_err());
        let input = quote! { alpha_price.saturating_mul(float_alpha_block_emission) };
        assert!(lint(input).is_err());
        let input = quote! { alpha_out_i.saturating_sub(root_alpha) };
        assert!(lint(input).is_err());
    }

    #[test]
    fn test_saturating_ufcs_forbidden() {
        let input = quote! { SaturatingAdd::saturating_add(stake, alpha) };
        assert!(lint(input).is_err());
        let input = quote! { core::num::SaturatingAdd::saturating_add(stake, alpha) };
        assert!(lint(input).is_err());
        let input =
            quote! { SaturatingMul::saturating_mul(alpha_price, float_alpha_block_emission) };
        assert!(lint(input).is_err());
        let input = quote! { core::num::SaturatingMul::saturating_mul(alpha_price, float_alpha_block_emission) };
        assert!(lint(input).is_err());
        let input = quote! { SaturatingSub::saturating_sub(alpha_out_i, root_alpha) };
        assert!(lint(input).is_err());
        let input = quote! { core::num::SaturatingSub::saturating_sub(alpha_out_i, root_alpha) };
        assert!(lint(input).is_err());
    }

    #[test]
    fn test_saturating_to_from_num_forbidden() {
        let input = quote! { I96F32::saturating_from_num(u64::MAX) };
        assert!(lint(input).is_err());
        let input = quote! { remaining_emission.saturating_to_num::<u64>() };
        assert!(lint(input).is_err());
    }
}
