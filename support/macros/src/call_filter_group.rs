use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    Error, Expr, Ident, Path, Result, Token, bracketed, parenthesized,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
};

/// Parsed input for one call filter group.
pub struct CallFilterGroupInput {
    group: Ident,
    rules: Punctuated<CallRule, Token![,]>,
}

/// One allowed call, optionally guarded by a simple condition.
struct CallRule {
    target: RuntimeCallRef,
    condition: Option<CallConstraint>,
}

/// Reference to `RuntimeCall::Variant(pallet::Call::method)`.
#[derive(Clone)]
struct RuntimeCallRef {
    variant: Ident,
    call_enum: Path,
    call: Ident,
}

/// Constraints supported by the generated filter and metadata.
enum CallConstraint {
    ParamLessThan {
        field: Ident,
        limit: Expr,
    },
    NestedCallMustBe {
        field: Ident,
        target: RuntimeCallRef,
    },
}

impl Parse for CallFilterGroupInput {
    fn parse(input: ParseStream) -> Result<Self> {
        let group = input.parse()?;
        input.parse::<Token![,]>()?;

        let content;
        bracketed!(content in input);
        let rules = content.parse_terminated(CallRule::parse, Token![,])?;

        Ok(Self { group, rules })
    }
}

impl Parse for CallRule {
    fn parse(input: ParseStream) -> Result<Self> {
        let target = input.parse()?;
        let condition = if input.peek(Token![where]) {
            input.parse::<Token![where]>()?;
            Some(input.parse()?)
        } else {
            None
        };

        Ok(Self { target, condition })
    }
}

impl Parse for RuntimeCallRef {
    fn parse(input: ParseStream) -> Result<Self> {
        let runtime_call = input.parse::<Ident>()?;
        if runtime_call != "RuntimeCall" {
            return Err(Error::new_spanned(
                runtime_call,
                "expected RuntimeCall::Variant(call_enum::Call::method)",
            ));
        }

        input.parse::<Token![::]>()?;
        let variant = input.parse()?;

        let content;
        parenthesized!(content in input);
        let call_path = content.parse()?;
        if !content.is_empty() {
            return Err(content.error("expected a single call path"));
        }

        let (call_enum, call) = split_call_path(call_path)?;
        Ok(Self {
            variant,
            call_enum,
            call,
        })
    }
}

impl Parse for CallConstraint {
    fn parse(input: ParseStream) -> Result<Self> {
        let field = input.parse::<Ident>()?;

        if field == "nested" {
            let content;
            parenthesized!(content in input);
            let nested_field = content.parse()?;
            if !content.is_empty() {
                return Err(content.error("expected a single nested call field"));
            }

            input.parse::<Token![==]>()?;
            let target = input.parse()?;

            return Ok(Self::NestedCallMustBe {
                field: nested_field,
                target,
            });
        }

        input.parse::<Token![<]>()?;
        let limit = input.parse()?;
        Ok(Self::ParamLessThan { field, limit })
    }
}

/// Generate both impls for a filter group:
/// - `Contains<RuntimeCall>` for execution-time filtering
/// - `CallFilterMetadata` for runtime API discovery
pub fn generate(input: CallFilterGroupInput) -> Result<TokenStream2> {
    let group = input.group;
    let contains_rules = input.rules.iter().map(generate_contains_rule);
    let call_infos = input.rules.iter().map(generate_call_info);

    Ok(quote! {
        pub(super) struct #group;

        impl frame_support::traits::Contains<crate::RuntimeCall> for #group {
            fn contains(call: &crate::RuntimeCall) -> bool {
                false #( || #contains_rules )*
            }
        }

        impl subtensor_runtime_common::CallFilterMetadata for #group {
            fn call_infos() -> ::alloc::vec::Vec<subtensor_runtime_common::CallInfo> {
                ::alloc::vec![#(#call_infos),*]
            }
        }
    })
}

/// Build the executable predicate for one allowed call.
fn generate_contains_rule(rule: &CallRule) -> TokenStream2 {
    match &rule.condition {
        None => {
            let pattern = call_pattern(&rule.target, None);
            quote! { matches!(call, #pattern) }
        }
        Some(CallConstraint::ParamLessThan { field, limit }) => {
            let pattern = call_pattern(&rule.target, Some(field));
            quote! {
                match call {
                    #pattern => *#field < #limit,
                    _ => false,
                }
            }
        }
        Some(CallConstraint::NestedCallMustBe { field, target }) => {
            let source_pattern = call_pattern(&rule.target, Some(field));
            let target_pattern = call_pattern(target, None);
            quote! {
                match call {
                    #source_pattern => matches!(#field.as_ref(), #target_pattern),
                    _ => false,
                }
            }
        }
    }
}

/// Build the metadata entry for one allowed call.
fn generate_call_info(rule: &CallRule) -> TokenStream2 {
    let base = call_info_expr(&rule.target);

    match &rule.condition {
        None => base,
        Some(CallConstraint::ParamLessThan { field, limit }) => quote! {{
            let mut info = #base;
            info.constraint = Some(subtensor_runtime_common::CallConstraint::ParamLessThan {
                param_name: stringify!(#field).as_bytes().to_vec(),
                limit: Into::<u64>::into(#limit) as u128,
            });
            info
        }},
        Some(CallConstraint::NestedCallMustBe { field, target }) => {
            let nested = call_info_expr(target);
            quote! {{
                let mut info = #base;
                let nested = #nested;
                info.constraint = Some(subtensor_runtime_common::CallConstraint::NestedCallMustBe {
                    param_name: stringify!(#field).as_bytes().to_vec(),
                    pallet_name: nested.pallet_name,
                    call_name: nested.call_name,
                });
                info
            }}
        }
    }
}

/// Convert a call reference into a `RuntimeCall` match pattern.
fn call_pattern(target: &RuntimeCallRef, field: Option<&Ident>) -> TokenStream2 {
    let variant = &target.variant;
    let call_enum = &target.call_enum;
    let call = &target.call;

    match field {
        Some(field) => quote! {
            crate::RuntimeCall::#variant(#call_enum::#call { #field, .. })
        },
        None => quote! {
            crate::RuntimeCall::#variant(#call_enum::#call { .. })
        },
    }
}

/// Convert a call reference into a runtime lookup for pallet/call indexes.
fn call_info_expr(target: &RuntimeCallRef) -> TokenStream2 {
    let variant = &target.variant;
    let call_enum = &target.call_enum;
    let call = &target.call;

    quote! {
        subtensor_runtime_common::call_info_by_name::<
            crate::#variant,
            #call_enum<crate::Runtime>,
        >(stringify!(#call))
    }
}

/// Split `pallet::Call::method` into `pallet::Call` and `method`.
fn split_call_path(call_path: Path) -> Result<(Path, Ident)> {
    if call_path.segments.len() < 2 {
        return Err(Error::new_spanned(
            call_path,
            "expected a call path like pallet_name::Call::method",
        ));
    }

    let mut call_enum_segments = Punctuated::new();
    for segment in call_path
        .segments
        .iter()
        .take(call_path.segments.len().saturating_sub(1))
    {
        call_enum_segments.push((*segment).clone());
    }

    #[allow(clippy::expect_used)]
    let call = call_path
        .segments
        .last()
        .expect("length checked above")
        .ident
        .clone();

    Ok((
        Path {
            leading_colon: call_path.leading_colon,
            segments: call_enum_segments,
        },
        call,
    ))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use quote::quote;

    use super::*;

    fn generate_tokens(input: TokenStream2) -> String {
        let input = syn::parse2::<CallFilterGroupInput>(input).unwrap();
        generate(input).unwrap().to_string()
    }

    #[test]
    fn parses_group_name_and_call_targets() {
        let input = syn::parse2::<CallFilterGroupInput>(quote! {
            StakingOperations, [
                RuntimeCall::SubtensorModule(pallet_subtensor::Call::add_stake),
                RuntimeCall::SubtensorModule(pallet_subtensor::Call::remove_stake),
            ]
        })
        .unwrap();

        assert_eq!(input.group, "StakingOperations");
        assert_eq!(input.rules.len(), 2);
        assert_eq!(input.rules[0].target.variant, "SubtensorModule");
        assert_eq!(input.rules[0].target.call, "add_stake");
        assert!(input.rules[0].condition.is_none());
    }

    #[test]
    fn generated_group_implements_contains_and_shared_metadata() {
        let generated = generate_tokens(quote! {
            StakingOperations, [
                RuntimeCall::SubtensorModule(pallet_subtensor::Call::add_stake),
            ]
        });

        assert!(generated.contains(
            "impl frame_support :: traits :: Contains < crate :: RuntimeCall > for StakingOperations"
        ));
        assert!(
            generated.contains(
                "impl subtensor_runtime_common :: CallFilterMetadata for StakingOperations"
            )
        );
        assert!(generated.contains(
            "matches ! (call , crate :: RuntimeCall :: SubtensorModule (pallet_subtensor :: Call :: add_stake"
        ));
        assert!(generated.contains("subtensor_runtime_common :: call_info_by_name"));
        assert!(generated.contains("crate :: SubtensorModule"));
        assert!(generated.contains("pallet_subtensor :: Call < crate :: Runtime >"));
        assert!(generated.contains("stringify ! (add_stake)"));
    }

    #[test]
    fn generated_group_supports_param_less_than_condition() {
        let generated = generate_tokens(quote! {
            SmallTransfers, [
                RuntimeCall::Balances(pallet_balances::Call::transfer_allow_death)
                    where value < SMALL_TRANSFER_LIMIT,
            ]
        });

        assert!(generated.contains("* value < SMALL_TRANSFER_LIMIT"));
        assert!(generated.contains("subtensor_runtime_common :: CallConstraint :: ParamLessThan"));
        assert!(generated.contains("param_name : stringify ! (value)"));
        assert!(
            generated.contains("limit : Into :: < u64 > :: into (SMALL_TRANSFER_LIMIT) as u128")
        );
    }

    #[test]
    fn generated_group_supports_nested_call_condition() {
        let generated = generate_tokens(quote! {
            SudoUncheckedSetCodeOperation, [
                RuntimeCall::Sudo(pallet_sudo::Call::sudo_unchecked_weight)
                    where nested(call) == RuntimeCall::System(frame_system::Call::set_code),
            ]
        });

        assert!(generated.contains(
            "matches ! (call . as_ref () , crate :: RuntimeCall :: System (frame_system :: Call :: set_code"
        ));
        assert!(
            generated.contains("subtensor_runtime_common :: CallConstraint :: NestedCallMustBe")
        );
        assert!(generated.contains("param_name : stringify ! (call)"));
        assert!(generated.contains("pallet_name : nested . pallet_name"));
        assert!(generated.contains("call_name : nested . call_name"));
    }

    #[test]
    fn rejects_non_runtime_call_target() {
        let err = match syn::parse2::<CallFilterGroupInput>(quote! {
            BadGroup, [
                Call::SubtensorModule(pallet_subtensor::Call::add_stake),
            ]
        }) {
            Ok(_) => panic!("invalid runtime call target should fail to parse"),
            Err(err) => err,
        };

        assert!(
            err.to_string()
                .contains("expected RuntimeCall::Variant(call_enum::Call::method)")
        );
    }

    #[test]
    fn rejects_call_paths_without_call_name() {
        let err = match syn::parse2::<CallFilterGroupInput>(quote! {
            BadGroup, [
                RuntimeCall::SubtensorModule(add_stake),
            ]
        }) {
            Ok(_) => panic!("call path without call enum should fail to parse"),
            Err(err) => err,
        };

        assert!(
            err.to_string()
                .contains("expected a call path like pallet_name::Call::method")
        );
    }
}
