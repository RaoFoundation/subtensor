mod call_groups;

use alloc::{format, vec::Vec};

use call_groups::*;
use frame_support::traits::{Contains, InstanceFilter};
use subtensor_runtime_common::{
    CallFilterMetadata, FilterMode, ProxyFilterInfo, ProxyType, ProxyTypeInfo,
};

use crate::RuntimeCall;

// ============================================================================
// Per-proxy allow-lists
//
// Each proxy type's permission set is an *additive* union of whole call groups
// from `call_groups`. A call a proxy does not list is denied. `Any` allows
// everything; the deprecated proxies allow nothing.
//
// `Contains` for a tuple is logical OR (any member matches), so these aliases
// read as "allow if the call is in any of these groups".
// ============================================================================

/// All admin-utils configuration. Every broad proxy historically allowed every
/// admin call; the root-only (`RootConfigCalls`) and owner-key (`OwnerKeyCalls`)
/// calls are gated by the dispatch's own origin check, so granting them to a
/// signed proxy is inert.
type AdminAll = (SubnetManagementCalls, RootConfigCalls, OwnerKeyCalls);

/// `Transfer`: liquid value movement.
type TransferAllowed = (BalanceTransferCalls, StakeTransferCalls);

/// `Staking`: stake position management plus root-claim mode selection.
type StakingAllowed = (StakeManagementCalls, RootClaimTypeCalls);

/// `Registration`: acquire a slot (POW or by burn).
type RegistrationAllowed = (PowRegistrationCalls, BurnedRegistrationCalls);

/// `Owner`: run a subnet you own — subnet identity plus the owner-settable
/// admin config. Excludes root-only admin (it can't pass `ensure_root`) and
/// owner-key rotation.
type OwnerAllowed = (SubnetIdentityCalls, SubnetManagementCalls);

/// `SubnetLeaseBeneficiary`: operate a leased subnet (activation, identity, and
/// the owner-settable subnet management config).
type SubnetLeaseAllowed = (
    SubnetActivationCalls,
    SubnetIdentityCalls,
    SubnetManagementCalls,
);

/// `NonTransfer`: everything except liquid value movement and coldkey swaps.
type NonTransferAllowed = (
    InfraCommonCalls,
    AdminAll,
    SudoCalls,
    StakeManagementCalls,
    PowRegistrationCalls,
    BurnedRegistrationCalls,
    FaucetCalls,
    RootRegistrationCalls,
    HotkeySwapCalls,
    CriticalNetworkCalls,
    ChildKeyCalls,
    RootClaimCalls,
    RootClaimTypeCalls,
    SubnetIdentityCalls,
    SubnetActivationCalls,
    SubtensorCommonCalls,
);

/// `NonFungible`: nothing that moves TAO/alpha and no key swaps.
type NonFungibleAllowed = (
    InfraCommonCalls,
    AdminAll,
    SudoCalls,
    PowRegistrationCalls,
    FaucetCalls,
    CriticalNetworkCalls,
    ChildKeyCalls,
    RootClaimCalls,
    RootClaimTypeCalls,
    SubnetIdentityCalls,
    SubnetActivationCalls,
    SubtensorCommonCalls,
);

/// `NonCritical`: day-to-day operations including value movement, but no sudo,
/// network dissolution, root/burned registration, or coldkey swaps.
type NonCriticalAllowed = (
    InfraCommonCalls,
    AdminAll,
    BalanceTransferCalls,
    BalanceMaintenanceCalls,
    StakeManagementCalls,
    StakeTransferCalls,
    PowRegistrationCalls,
    FaucetCalls,
    HotkeySwapCalls,
    ChildKeyCalls,
    RootClaimCalls,
    RootClaimTypeCalls,
    SubnetIdentityCalls,
    SubnetActivationCalls,
    SubtensorCommonCalls,
);

pub(crate) fn proxy_type_filter(proxy_type: &ProxyType, call: &RuntimeCall) -> bool {
    match proxy_type {
        ProxyType::Any => true,
        ProxyType::Owner => OwnerAllowed::contains(call),
        ProxyType::NonCritical => NonCriticalAllowed::contains(call),
        ProxyType::NonTransfer => NonTransferAllowed::contains(call),
        ProxyType::NonFungible => NonFungibleAllowed::contains(call),
        ProxyType::Staking => StakingAllowed::contains(call),
        ProxyType::Registration => RegistrationAllowed::contains(call),
        ProxyType::Transfer => TransferAllowed::contains(call),
        ProxyType::SmallTransfer => SmallTransferCalls::contains(call),
        ProxyType::ChildKeys => ChildKeyCalls::contains(call),
        ProxyType::SwapHotkey => HotkeySwapCalls::contains(call),
        ProxyType::SubnetLeaseBeneficiary => SubnetLeaseAllowed::contains(call),
        ProxyType::RootClaim => RootClaimCalls::contains(call),
        ProxyType::SudoUncheckedSetCode => SudoSetCodeCalls::contains(call),
        ProxyType::Triumvirate
        | ProxyType::Senate
        | ProxyType::Governance
        | ProxyType::RootWeights => false,
    }
}

impl InstanceFilter<RuntimeCall> for ProxyType {
    fn filter(&self, call: &RuntimeCall) -> bool {
        proxy_type_filter(self, call)
    }

    fn is_superset(&self, other: &Self) -> bool {
        match (self, other) {
            (x, y) if x == y => true,
            (ProxyType::Any, _) => true,
            (_, ProxyType::Any) => false,
            // Keep this positive list explicit. A future proxy type that can
            // move value must not become addable through `NonTransfer` by
            // default.
            (
                ProxyType::NonTransfer,
                ProxyType::Owner
                | ProxyType::Senate
                | ProxyType::NonFungible
                | ProxyType::Triumvirate
                | ProxyType::Governance
                | ProxyType::Staking
                | ProxyType::Registration
                | ProxyType::RootWeights
                | ProxyType::ChildKeys
                | ProxyType::SudoUncheckedSetCode
                | ProxyType::SwapHotkey
                | ProxyType::SubnetLeaseBeneficiary
                | ProxyType::RootClaim,
            ) => true,
            (ProxyType::Transfer, ProxyType::SmallTransfer) => true,
            _ => false,
        }
    }
}

// ============================================================================
// Runtime API metadata
//
// The client-facing allowlist view is derived from the same call groups the
// filter uses, so the two cannot drift.
// ============================================================================

/// The filter mode (allow-all or an explicit allowlist) for one proxy type.
fn proxy_filter_mode(proxy_type: ProxyType) -> FilterMode {
    match proxy_type {
        ProxyType::Any => FilterMode::AllowAll,
        ProxyType::Owner => FilterMode::Allow(OwnerAllowed::call_infos()),
        ProxyType::NonCritical => FilterMode::Allow(NonCriticalAllowed::call_infos()),
        ProxyType::NonTransfer => FilterMode::Allow(NonTransferAllowed::call_infos()),
        ProxyType::NonFungible => FilterMode::Allow(NonFungibleAllowed::call_infos()),
        ProxyType::Staking => FilterMode::Allow(StakingAllowed::call_infos()),
        ProxyType::Registration => FilterMode::Allow(RegistrationAllowed::call_infos()),
        ProxyType::Transfer => FilterMode::Allow(TransferAllowed::call_infos()),
        ProxyType::SmallTransfer => FilterMode::Allow(SmallTransferCalls::call_infos()),
        ProxyType::ChildKeys => FilterMode::Allow(ChildKeyCalls::call_infos()),
        ProxyType::SwapHotkey => FilterMode::Allow(HotkeySwapCalls::call_infos()),
        ProxyType::SubnetLeaseBeneficiary => FilterMode::Allow(SubnetLeaseAllowed::call_infos()),
        ProxyType::RootClaim => FilterMode::Allow(RootClaimCalls::call_infos()),
        ProxyType::SudoUncheckedSetCode => FilterMode::Allow(SudoSetCodeCalls::call_infos()),
        ProxyType::Triumvirate
        | ProxyType::Senate
        | ProxyType::Governance
        | ProxyType::RootWeights => FilterMode::Allow(Vec::new()),
    }
}

/// Every proxy type with its on-chain index and deprecation flag.
pub fn get_all_proxy_type_infos() -> Vec<ProxyTypeInfo> {
    (0u8..=u8::MAX)
        .filter_map(|index| {
            ProxyType::try_from(index)
                .ok()
                .map(|proxy_type| ProxyTypeInfo {
                    name: format!("{:?}", proxy_type).into_bytes(),
                    index,
                    deprecated: proxy_type.is_deprecated(),
                })
        })
        .collect()
}

/// Filter metadata for the requested proxy types (all of them when `None`).
pub fn get_proxy_filters(proxy_types: Option<Vec<u8>>) -> Vec<ProxyFilterInfo> {
    (0u8..=u8::MAX)
        .filter_map(|index| {
            ProxyType::try_from(index)
                .ok()
                .map(|proxy_type| (index, proxy_type))
        })
        .filter(|(index, _)| {
            proxy_types
                .as_ref()
                .is_none_or(|selected| selected.contains(index))
        })
        .map(|(index, proxy_type)| ProxyFilterInfo {
            proxy_type: index,
            name: format!("{:?}", proxy_type).into_bytes(),
            deprecated: proxy_type.is_deprecated(),
            filter_mode: proxy_filter_mode(proxy_type),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use alloc::{
        collections::BTreeSet,
        string::{String, ToString},
        vec,
    };
    use frame_support::traits::GetCallMetadata;
    use subtensor_runtime_common::CallInfo;

    fn call_name(info: &CallInfo) -> String {
        format!(
            "{}::{}",
            String::from_utf8_lossy(&info.pallet_name),
            String::from_utf8_lossy(&info.call_name)
        )
    }

    /// All `pallet::call` names in the runtime, straight from `RuntimeCall`
    /// metadata.
    fn all_runtime_calls() -> BTreeSet<String> {
        RuntimeCall::get_module_names()
            .iter()
            .flat_map(|module| {
                RuntimeCall::get_call_names(module)
                    .iter()
                    .map(move |call| format!("{}::{}", module, call))
            })
            .collect()
    }

    fn group_calls<G: CallFilterMetadata>() -> BTreeSet<String> {
        G::call_infos().iter().map(call_name).collect()
    }

    /// The set of calls a proxy type allows, taken from its metadata view.
    fn allowed_calls(proxy_type: ProxyType) -> BTreeSet<String> {
        match proxy_filter_mode(proxy_type) {
            FilterMode::AllowAll => all_runtime_calls(),
            FilterMode::Allow(infos) => infos.iter().map(call_name).collect(),
        }
    }

    fn expected(calls: &[&str]) -> BTreeSet<String> {
        calls.iter().map(|c| c.to_string()).collect()
    }

    fn all_proxy_types() -> Vec<ProxyType> {
        (0u8..=u8::MAX)
            .filter_map(|index| ProxyType::try_from(index).ok())
            .collect()
    }

    #[test]
    fn any_allows_everything_and_deprecated_allow_nothing() {
        assert_eq!(allowed_calls(ProxyType::Any), all_runtime_calls());
        for deprecated in [
            ProxyType::Triumvirate,
            ProxyType::Senate,
            ProxyType::Governance,
            ProxyType::RootWeights,
        ] {
            assert!(allowed_calls(deprecated).is_empty());
        }
    }

    // Broad proxies are specified subtractively here (all calls minus a few
    // denied groups) and checked against the additive composition in the filter.
    // Because the inventory groups partition every runtime call, the two must
    // agree exactly; a missing or extra group in the filter shows up as a diff.
    #[test]
    fn non_transfer_is_everything_but_transfers_and_coldkey_swaps() {
        let denied = &(&group_calls::<BalanceTransferCalls>()
            | &group_calls::<BalanceMaintenanceCalls>())
            | &(&group_calls::<StakeTransferCalls>() | &group_calls::<ColdkeySwapCalls>());
        assert_eq!(
            allowed_calls(ProxyType::NonTransfer),
            &all_runtime_calls() - &denied
        );
    }

    #[test]
    fn non_fungible_is_everything_but_value_movement_and_key_swaps() {
        let denied = &(&(&group_calls::<BalanceTransferCalls>()
            | &group_calls::<BalanceMaintenanceCalls>())
            | &(&group_calls::<StakeManagementCalls>() | &group_calls::<StakeTransferCalls>()))
            | &(&(&group_calls::<BurnedRegistrationCalls>()
                | &group_calls::<RootRegistrationCalls>())
                | &(&group_calls::<HotkeySwapCalls>() | &group_calls::<ColdkeySwapCalls>()));
        assert_eq!(
            allowed_calls(ProxyType::NonFungible),
            &all_runtime_calls() - &denied
        );
    }

    #[test]
    fn non_critical_is_everything_but_sudo_and_critical_ops() {
        let denied = &(&(&group_calls::<SudoCalls>() | &group_calls::<BurnedRegistrationCalls>())
            | &(&group_calls::<RootRegistrationCalls>() | &group_calls::<CriticalNetworkCalls>()))
            | &group_calls::<ColdkeySwapCalls>();
        assert_eq!(
            allowed_calls(ProxyType::NonCritical),
            &all_runtime_calls() - &denied
        );
    }

    #[test]
    fn superset_relations_match_allowed_call_sets() {
        let proxy_types = all_proxy_types();
        let mut violations = Vec::new();

        for parent in &proxy_types {
            let parent_calls = allowed_calls(*parent);
            for child in &proxy_types {
                if !parent.is_superset(child) {
                    continue;
                }

                let child_calls = allowed_calls(*child);
                let missing = child_calls
                    .difference(&parent_calls)
                    .cloned()
                    .collect::<Vec<_>>();

                if !missing.is_empty() {
                    violations.push(format!(
                        "{:?}.is_superset({:?}) is missing:\n{}",
                        parent,
                        child,
                        missing
                            .iter()
                            .map(|call| format!("  {}", call))
                            .collect::<Vec<_>>()
                            .join("\n")
                    ));
                }
            }
        }

        assert!(
            violations.is_empty(),
            "is_superset claims proxy permissions that the parent filter does not allow:\n{}",
            violations.join("\n\n")
        );
    }

    #[test]
    fn non_transfer_superset_is_explicit_allowlist() {
        let actual = all_proxy_types()
            .into_iter()
            .filter(|proxy_type| ProxyType::NonTransfer.is_superset(proxy_type))
            .collect::<BTreeSet<_>>();
        let expected = [
            ProxyType::Owner,
            ProxyType::NonTransfer,
            ProxyType::Senate,
            ProxyType::NonFungible,
            ProxyType::Triumvirate,
            ProxyType::Governance,
            ProxyType::Staking,
            ProxyType::Registration,
            ProxyType::RootWeights,
            ProxyType::ChildKeys,
            ProxyType::SudoUncheckedSetCode,
            ProxyType::SwapHotkey,
            ProxyType::SubnetLeaseBeneficiary,
            ProxyType::RootClaim,
        ]
        .into_iter()
        .collect::<BTreeSet<_>>();

        assert_eq!(actual, expected);
    }

    #[test]
    fn owner_allows_only_owner_settable_config() {
        let owner = allowed_calls(ProxyType::Owner);
        // Owner-settable subnet params + subnet identity.
        assert!(owner.contains("AdminUtils::sudo_set_serving_rate_limit"));
        assert!(owner.contains("AdminUtils::sudo_set_max_difficulty"));
        assert!(owner.contains("SubtensorModule::set_subnet_identity"));
        // Root-only admin is not owner-settable (gated by `ensure_root`).
        assert!(!owner.contains("AdminUtils::sudo_set_tempo"));
        assert!(!owner.contains("AdminUtils::sudo_set_kappa"));
        assert!(!owner.contains("AdminUtils::sudo_set_total_issuance"));
        assert!(!owner.contains("AdminUtils::swap_authorities"));
        // Never owner-key rotation.
        assert!(!owner.contains("AdminUtils::sudo_set_sn_owner_hotkey"));
        // Exactly subnet identity plus the owner-settable management config.
        let expected =
            &group_calls::<SubnetIdentityCalls>() | &group_calls::<SubnetManagementCalls>();
        assert_eq!(owner, expected);
    }

    #[test]
    fn subnet_lease_boundaries() {
        let lease = allowed_calls(ProxyType::SubnetLeaseBeneficiary);
        // Can activate and tune the subnet's owner-settable params...
        assert!(lease.contains("SubtensorModule::start_call"));
        assert!(lease.contains("SubtensorModule::set_subnet_identity"));
        assert!(lease.contains("AdminUtils::sudo_set_serving_rate_limit"));
        // ...but not root-only params, owner keys, authorities, or lease teardown.
        assert!(!lease.contains("AdminUtils::sudo_set_kappa"));
        assert!(!lease.contains("AdminUtils::sudo_set_total_issuance"));
        assert!(!lease.contains("AdminUtils::sudo_set_sn_owner_hotkey"));
        assert!(!lease.contains("AdminUtils::swap_authorities"));
        assert!(!lease.contains("SubtensorModule::terminate_lease"));
    }

    #[test]
    fn narrow_proxies_have_exact_allow_lists() {
        assert_eq!(
            allowed_calls(ProxyType::Transfer),
            expected(&[
                "Balances::transfer_keep_alive",
                "Balances::transfer_allow_death",
                "Balances::transfer_all",
                "SubtensorModule::transfer_stake",
            ])
        );
        assert_eq!(
            allowed_calls(ProxyType::SmallTransfer),
            expected(&[
                "Balances::transfer_keep_alive",
                "Balances::transfer_allow_death",
                "SubtensorModule::transfer_stake",
            ])
        );
        assert_eq!(
            allowed_calls(ProxyType::Staking),
            expected(&[
                "SubtensorModule::add_stake",
                "SubtensorModule::add_stake_limit",
                "SubtensorModule::remove_stake",
                "SubtensorModule::remove_stake_limit",
                "SubtensorModule::remove_stake_full_limit",
                "SubtensorModule::unstake_all",
                "SubtensorModule::unstake_all_alpha",
                "SubtensorModule::move_stake",
                "SubtensorModule::swap_stake",
                "SubtensorModule::swap_stake_limit",
                "SubtensorModule::set_root_claim_type",
            ])
        );
        assert_eq!(
            allowed_calls(ProxyType::Registration),
            expected(&[
                "SubtensorModule::register",
                "SubtensorModule::register_limit",
                "SubtensorModule::burned_register",
            ])
        );
        assert_eq!(
            allowed_calls(ProxyType::ChildKeys),
            expected(&[
                "SubtensorModule::set_children",
                "SubtensorModule::set_childkey_take",
            ])
        );
        assert_eq!(
            allowed_calls(ProxyType::SwapHotkey),
            expected(&[
                "SubtensorModule::swap_hotkey",
                "SubtensorModule::swap_hotkey_v2",
            ])
        );
        assert_eq!(
            allowed_calls(ProxyType::RootClaim),
            expected(&["SubtensorModule::claim_root"])
        );
        assert_eq!(
            allowed_calls(ProxyType::SudoUncheckedSetCode),
            expected(&["Sudo::sudo_unchecked_weight"])
        );
    }

    // The newer calls that leaked through `main`'s denylists must stay denied
    // for every broad proxy.
    #[test]
    fn tightened_denylist_leaks_stay_denied() {
        for proxy_type in [
            ProxyType::NonTransfer,
            ProxyType::NonFungible,
            ProxyType::NonCritical,
        ] {
            let allowed = allowed_calls(proxy_type);
            assert!(!allowed.contains("SubtensorModule::reset_coldkey_swap"));
            assert!(!allowed.contains("SubtensorModule::swap_coldkey"));
            assert!(!allowed.contains("SubtensorModule::schedule_swap_coldkey"));
        }
        // `root_dissolve_network` leaked into NonCritical specifically.
        assert!(
            !allowed_calls(ProxyType::NonCritical)
                .contains("SubtensorModule::root_dissolve_network")
        );
    }

    // The SmallTransfer / SudoUncheckedSetCode metadata must carry their
    // amount / nested-call constraints.
    #[test]
    fn conditional_proxies_expose_constraints() {
        use subtensor_runtime_common::CallConstraint;

        let small = match proxy_filter_mode(ProxyType::SmallTransfer) {
            FilterMode::Allow(infos) => infos,
            FilterMode::AllowAll => vec![],
        };
        assert!(
            small
                .iter()
                .all(|info| matches!(info.constraint, Some(CallConstraint::ParamLessThan { .. })))
        );

        let set_code = match proxy_filter_mode(ProxyType::SudoUncheckedSetCode) {
            FilterMode::Allow(infos) => infos,
            FilterMode::AllowAll => vec![],
        };
        assert!(set_code.iter().any(|info| matches!(
            &info.constraint,
            Some(CallConstraint::NestedCallMustBe { pallet_name, call_name, .. })
            if pallet_name == b"System" && call_name == b"set_code"
        )));
    }

    // The name-based golden tests above don't exercise the amount / nested-call
    // predicates, so check them directly through the filter.
    #[test]
    fn small_transfer_enforces_amount_limits() {
        use frame_system::Call as SystemCall;
        use pallet_balances::Call as BalancesCall;
        use pallet_subtensor::Call as SubtensorCall;
        use subtensor_runtime_common::{
            AccountId, AlphaBalance, NetUid, SMALL_ALPHA_TRANSFER_LIMIT, SMALL_TRANSFER_LIMIT,
            TaoBalance,
        };

        let dest = AccountId::new([2u8; 32]);

        let balance_transfer = |value: TaoBalance| {
            RuntimeCall::Balances(BalancesCall::transfer_allow_death {
                dest: dest.clone().into(),
                value,
            })
        };
        let stake_transfer = |alpha_amount: AlphaBalance| {
            RuntimeCall::SubtensorModule(SubtensorCall::transfer_stake {
                destination_coldkey: dest.clone(),
                hotkey: dest.clone(),
                origin_netuid: NetUid::from(1),
                destination_netuid: NetUid::from(1),
                alpha_amount,
            })
        };

        // Strictly-below the limit is allowed; at the limit is denied.
        assert!(proxy_type_filter(
            &ProxyType::SmallTransfer,
            &balance_transfer(TaoBalance::from(1))
        ));
        assert!(!proxy_type_filter(
            &ProxyType::SmallTransfer,
            &balance_transfer(SMALL_TRANSFER_LIMIT)
        ));
        assert!(proxy_type_filter(
            &ProxyType::SmallTransfer,
            &stake_transfer(AlphaBalance::from(1))
        ));
        assert!(!proxy_type_filter(
            &ProxyType::SmallTransfer,
            &stake_transfer(SMALL_ALPHA_TRANSFER_LIMIT)
        ));

        // A non-transfer call is never a small transfer.
        let remark = RuntimeCall::System(SystemCall::remark { remark: vec![] });
        assert!(!proxy_type_filter(&ProxyType::SmallTransfer, &remark));

        // `Transfer` is unconditional: the at-limit amount still passes.
        assert!(proxy_type_filter(
            &ProxyType::Transfer,
            &balance_transfer(SMALL_TRANSFER_LIMIT)
        ));
    }

    #[test]
    fn sudo_unchecked_set_code_only_matches_set_code() {
        use alloc::boxed::Box;
        use frame_support::weights::Weight;
        use frame_system::Call as SystemCall;
        use pallet_sudo::Call as SudoCall;

        let unchecked = |inner: RuntimeCall| {
            RuntimeCall::Sudo(SudoCall::sudo_unchecked_weight {
                call: Box::new(inner),
                weight: Weight::zero(),
            })
        };
        let set_code = RuntimeCall::System(SystemCall::set_code { code: vec![] });
        let remark = RuntimeCall::System(SystemCall::remark { remark: vec![] });

        // Allowed only when wrapping `System::set_code`.
        assert!(proxy_type_filter(
            &ProxyType::SudoUncheckedSetCode,
            &unchecked(set_code.clone())
        ));
        assert!(!proxy_type_filter(
            &ProxyType::SudoUncheckedSetCode,
            &unchecked(remark)
        ));
        // `Sudo::sudo` (checked) never matches, even wrapping set_code.
        let checked = RuntimeCall::Sudo(SudoCall::sudo {
            call: Box::new(set_code),
        });
        assert!(!proxy_type_filter(
            &ProxyType::SudoUncheckedSetCode,
            &checked
        ));
    }
}
