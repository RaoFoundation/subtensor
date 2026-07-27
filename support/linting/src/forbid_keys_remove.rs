//! Ban `Keys::<T>::remove(netuid, uid)` — it can break the dense neuron uid sequence.
//!
//! Prefer `SubtensorModule::replace_neuron()` when swapping neurons. Opt out with `#[allow(unknown_lints)]`.

use super::*;
use syn::{
    Expr, ExprCall, ExprPath, File, Path, punctuated::Punctuated, spanned::Spanned, token::Comma,
    visit::Visit,
};

/// Lint: reject `Keys::<T>::remove(netuid, uid)` calls that can corrupt neuron ordering.
pub struct ForbidKeysRemoveCall;

impl Lint for ForbidKeysRemoveCall {
    fn lint(source: &File) -> Result {
        let mut visitor = KeysRemoveVisitor::default();
        visitor.visit_file(source);

        if visitor.errors.is_empty() {
            Ok(())
        } else {
            Err(visitor.errors)
        }
    }
}

#[derive(Default)]
struct KeysRemoveVisitor {
    errors: Vec<syn::Error>,
}

impl<'ast> Visit<'ast> for KeysRemoveVisitor {
    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        let ExprCall {
            func, args, attrs, ..
        } = node;

        if is_keys_remove_call(func, args) && !is_allowed(attrs) {
            let msg = "Keys::<T>::remove()` is banned to prevent accidentally breaking \
                the neuron sequence. If you need to replace neurons, try `SubtensorModule::replace_neuron()`";
            self.errors.push(syn::Error::new(node.func.span(), msg));
        }
    }
}

fn is_keys_remove_call(func: &Expr, args: &Punctuated<Expr, Comma>) -> bool {
    let Expr::Path(ExprPath {
        path: Path { segments: func, .. },
        ..
    }) = func
    else {
        return false;
    };

    func.len() == 2
        && args.len() == 2
        && func[0].ident == "Keys"
        && !func[0].arguments.is_none()
        && func[1].ident == "remove"
        && func[1].arguments.is_none()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;
    use quote::quote;

    fn lint(input: proc_macro2::TokenStream) -> Result {
        let mut visitor = KeysRemoveVisitor::default();
        let expr: syn::ExprCall = syn::parse2(input).expect("should be a valid function call");
        visitor.visit_expr_call(&expr);

        if visitor.errors.is_empty() {
            Ok(())
        } else {
            Err(visitor.errors)
        }
    }

    #[test]
    fn test_keys_remove_forbidden() {
        let input = quote! { Keys::<T>::remove(netuid, uid_to_replace) };
        assert!(lint(input).is_err());
        let input = quote! { Keys::<U>::remove(netuid, uid_to_replace) };
        assert!(lint(input).is_err());
        let input = quote! { Keys::<U>::remove(1, "2".parse().unwrap(),) };
        assert!(lint(input).is_err());
    }

    #[test]
    fn test_non_keys_remove_not_forbidden() {
        let input = quote! { remove(netuid, uid_to_replace) };
        assert!(lint(input).is_ok());
        let input = quote! { Keys::remove(netuid, uid_to_replace) };
        assert!(lint(input).is_ok());
        let input = quote! { Keys::<T>::remove::<U>(netuid, uid_to_replace) };
        assert!(lint(input).is_ok());
        let input = quote! { Keys::<T>::remove(netuid, uid_to_replace, third_wheel) };
        assert!(lint(input).is_ok());
        let input = quote! { ParentKeys::remove(netuid, uid_to_replace) };
        assert!(lint(input).is_ok());
        let input = quote! { ChildKeys::<T>::remove(netuid, uid_to_replace) };
        assert!(lint(input).is_ok());
    }

    #[test]
    fn test_keys_remove_allowed() {
        let input = quote! {
            #[allow(unknown_lints)]
            Keys::<T>::remove(netuid, uid_to_replace)
        };
        assert!(lint(input).is_ok());
        let input = quote! {
            #[allow(unknown_lints)]
            Keys::<U>::remove(netuid, uid_to_replace)
        };
        assert!(lint(input).is_ok());
        let input = quote! {
            #[allow(unknown_lints)]
            Keys::<U>::remove(1, "2".parse().unwrap(),)
        };
        assert!(lint(input).is_ok());
    }
}
