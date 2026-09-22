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

/// Admin-utils configuration granted to broad proxies. Root-only
/// (`RootConfigCalls`) calls are still inert (they need `ensure_root`).
/// Owner-key rotation is **not** inert: `sudo_set_sn_owner_hotkey` is
/// owner-or-root, so `OwnerKeyCalls` stays inventory-only.
type AdminAll = (SubnetManagementCalls, RootConfigCalls);

/// `Transfer`: liquid value movement.
type TransferAllowed = (BalanceTransferCalls, StakeTransferCalls);

/// `Staking`: stake position management.
type StakingAllowed = StakeManagementCalls;

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

/// `Weights`: submit weights for the real hotkey and nothing else. The
/// least-privilege grant for a third-party weight setter: it cannot re-point
/// the axon or the EVM key, so it cannot redirect where the subnet reaches or
/// pays the neuron.
type WeightsAllowed = WeightCalls;

/// `Validate`: run a validator hotkey from an operations key. Weights, axon /
/// prometheus serving, EVM-key association, and commitments. No stake, no
/// transfers, no key rotation, no child keys, no take changes, no wrappers
/// (`Utility`, `Proxy`, `Multisig`), so the delegate can never move or lock the
/// real's TAO or alpha. Carries forward `ProxyType::Validate` from PR #2543
/// (ppolewicz) and PR #3041 (cisterciansis), re-indexed after `BasketTrading`.
type ValidateAllowed = (WeightCalls, NeuronServingCalls, CommitmentCalls);

/// `NonTransfer`: excludes liquid value movement, coldkey swaps, sudo, EVM,
/// Contracts, and Crowdloan calls that can move value indirectly, Multisig
/// wrappers (they re-dispatch on a fresh origin that drops this filter),
/// `MevShield::store_encrypted` (decrypt-and-dispatch drops the filter),
/// owner-key rotation, and basket trading (which needs the explicit
/// `BasketTrading` grant). Sudo is excluded because a sudo-key principal
/// would otherwise hand the delegate root, including forced transfers.
/// `SudoCalls` is inventory-only: no restricted proxy grants it.
type NonTransferAllowed = (
    InfraCommonCalls,
    AdminAll,
    StakeManagementCalls,
    PowRegistrationCalls,
    BurnedRegistrationCalls,
    FaucetCalls,
    RootRegistrationCalls,
    HotkeySwapCalls,
    CriticalNetworkCalls,
    ChildKeyCalls,
    RootClaimCalls,
    SubnetIdentityCalls,
    SubnetActivationCalls,
    SubtensorValueCalls,
    WeightCalls,
    NeuronServingCalls,
    SubtensorCommonCalls,
);

/// `NonFungible`: nothing that moves, locks, burns or spends TAO/alpha, no key
/// swaps, no sudo, no Multisig wrappers (fresh-origin re-dispatch), no
/// `store_encrypted`, and no owner-key rotation.
type NonFungibleAllowed = (
    InfraCommonCalls,
    AdminAll,
    PowRegistrationCalls,
    FaucetCalls,
    CriticalNetworkCalls,
    ChildKeyCalls,
    RootClaimCalls,
    SubnetIdentityCalls,
    SubnetActivationCalls,
    WeightCalls,
    NeuronServingCalls,
    SubtensorCommonCalls,
);

/// `NonCritical`: day-to-day operations including value movement, but no sudo,
/// network dissolution, root/burned registration, coldkey swaps, Crowdloan
/// wrappers (they re-dispatch a caller-supplied call as the real coldkey),
/// Multisig wrappers, `store_encrypted`, owner-key rotation, or basket
/// trading (which needs the explicit `BasketTrading` grant).
type NonCriticalAllowed = (
    InfraCommonCalls,
    EvmCalls,
    ContractsCalls,
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
    SubnetIdentityCalls,
    SubnetActivationCalls,
    SubtensorValueCalls,
    WeightCalls,
    NeuronServingCalls,
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
        ProxyType::BasketTrading => BasketTradingCalls::contains(call),
        ProxyType::Validate => ValidateAllowed::contains(call),
        ProxyType::Weights => WeightsAllowed::contains(call),
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
            // default. `SudoUncheckedSetCode` is not listed: `NonTransfer`
            // no longer reaches Sudo.
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
                | ProxyType::SwapHotkey
                | ProxyType::SubnetLeaseBeneficiary
                | ProxyType::RootClaim
                | ProxyType::Validate
                | ProxyType::Weights,
            ) => true,
            (ProxyType::Transfer, ProxyType::SmallTransfer) => true,
            (ProxyType::Validate, ProxyType::Weights) => true,
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
        ProxyType::BasketTrading => FilterMode::Allow(BasketTradingCalls::call_infos()),
        ProxyType::Validate => FilterMode::Allow(ValidateAllowed::call_infos()),
        ProxyType::Weights => FilterMode::Allow(WeightsAllowed::call_infos()),
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
    fn non_transfer_excludes_transfers_coldkey_swaps_sudo_and_indirect_value_pallets() {
        let denied = &(&group_calls::<BalanceTransferCalls>()
            | &group_calls::<BalanceMaintenanceCalls>())
            | &(&group_calls::<StakeTransferCalls>() | &group_calls::<ColdkeySwapCalls>());
        let denied = &denied | &group_calls::<(EvmCalls, ContractsCalls, CrowdloanCalls)>();
        let denied = &denied | &group_calls::<(SudoCalls, MultisigCalls)>();
        let denied = &denied | &group_calls::<BasketTradingCalls>();
        let denied = &denied | &group_calls::<(MevShieldStoreEncryptedCalls, OwnerKeyCalls)>();
        assert_eq!(
            allowed_calls(ProxyType::NonTransfer),
            &all_runtime_calls() - &denied
        );
    }

    #[test]
    fn non_fungible_is_everything_but_value_movement_key_swaps_and_sudo() {
        let denied = &(&(&group_calls::<BalanceTransferCalls>()
            | &group_calls::<BalanceMaintenanceCalls>())
            | &(&group_calls::<StakeManagementCalls>() | &group_calls::<StakeTransferCalls>()))
            | &(&(&group_calls::<BurnedRegistrationCalls>()
                | &group_calls::<RootRegistrationCalls>())
                | &(&group_calls::<HotkeySwapCalls>() | &group_calls::<ColdkeySwapCalls>()));
        let denied = &denied | &group_calls::<(EvmCalls, ContractsCalls, CrowdloanCalls)>();
        let denied = &denied | &group_calls::<(SubtensorValueCalls, SudoCalls)>();
        let denied = &denied | &group_calls::<MultisigCalls>();
        let denied = &denied | &group_calls::<BasketTradingCalls>();
        let denied = &denied | &group_calls::<(MevShieldStoreEncryptedCalls, OwnerKeyCalls)>();
        assert_eq!(
            allowed_calls(ProxyType::NonFungible),
            &all_runtime_calls() - &denied
        );
    }

    /// Executable-filter proof (the pallet mock allows everything, so this must live in
    /// the runtime): `NonFungible` cannot dispatch the calls that spend, lock or destroy the
    /// principal's TAO/alpha, neither `NonTransfer` nor `NonFungible` can reach Sudo, and
    /// ordinary non-fungible operations still pass. Sudo-capable value-moving delegation
    /// also stays refused through the metadata the runtime API advertises.
    #[test]
    fn non_fungible_cannot_move_value_and_no_non_financial_proxy_reaches_sudo() {
        use subtensor_runtime_common::{AccountId, AlphaBalance, NetUid, TaoBalance};

        let hotkey = AccountId::new([7; 32]);
        let netuid = NetUid::from(1);
        let value_calls = [
            RuntimeCall::SubtensorModule(pallet_subtensor::Call::add_stake_burn {
                hotkey: hotkey.clone(),
                netuid,
                amount: TaoBalance::from(1),
                limit: None,
            }),
            RuntimeCall::SubtensorModule(pallet_subtensor::Call::lock_stake {
                hotkey: hotkey.clone(),
                netuid,
                amount: AlphaBalance::from(1),
            }),
            RuntimeCall::SubtensorModule(pallet_subtensor::Call::move_lock {
                destination_hotkey: hotkey.clone(),
                netuid,
            }),
            RuntimeCall::SubtensorModule(pallet_subtensor::Call::set_perpetual_lock {
                netuid,
                enabled: true,
            }),
            RuntimeCall::SubtensorModule(pallet_subtensor::Call::recycle_alpha {
                hotkey: hotkey.clone(),
                amount: AlphaBalance::from(1),
                netuid,
            }),
            RuntimeCall::SubtensorModule(pallet_subtensor::Call::burn_alpha {
                hotkey: hotkey.clone(),
                amount: AlphaBalance::from(1),
                netuid,
            }),
            RuntimeCall::SubtensorModule(pallet_subtensor::Call::register_network {
                hotkey: hotkey.clone(),
            }),
            RuntimeCall::SubtensorModule(pallet_subtensor::Call::register_network_with_identity {
                hotkey: hotkey.clone(),
                identity: None,
            }),
            RuntimeCall::SubtensorModule(pallet_subtensor::Call::register_leased_network {
                emissions_share: sp_runtime::Percent::from_percent(10),
                end_block: None,
            }),
        ];
        let sudo_calls = [
            RuntimeCall::Sudo(pallet_sudo::Call::sudo {
                call: alloc::boxed::Box::new(RuntimeCall::Balances(
                    pallet_balances::Call::force_transfer {
                        source: hotkey.clone().into(),
                        dest: hotkey.clone().into(),
                        value: TaoBalance::from(1),
                    },
                )),
            }),
            RuntimeCall::Sudo(pallet_sudo::Call::set_key {
                new: hotkey.clone().into(),
            }),
        ];
        let non_fungible_calls = [
            RuntimeCall::SubtensorModule(pallet_subtensor::Call::set_weights {
                netuid,
                dests: vec![0],
                weights: vec![1],
                version_key: 0,
            }),
            RuntimeCall::SubtensorModule(pallet_subtensor::Call::serve_axon {
                netuid,
                version: 1,
                ip: 1,
                port: 1,
                ip_type: 4,
                protocol: 0,
                placeholder1: 0,
                placeholder2: 0,
            }),
            RuntimeCall::SubtensorModule(pallet_subtensor::Call::set_reject_locked_alpha {
                enabled: true,
            }),
        ];

        for call in &value_calls {
            let name = call.get_call_metadata().function_name;
            assert!(
                !ProxyType::NonFungible.filter(call),
                "NonFungible must not dispatch {name}"
            );
            assert!(
                ProxyType::NonTransfer.filter(call),
                "NonTransfer keeps {name}"
            );
            assert!(
                ProxyType::NonCritical.filter(call),
                "NonCritical keeps {name}"
            );
        }
        for call in &sudo_calls {
            let name = call.get_call_metadata().function_name;
            for proxy_type in [
                ProxyType::NonTransfer,
                ProxyType::NonFungible,
                ProxyType::NonCritical,
            ] {
                assert!(
                    !proxy_type.filter(call),
                    "{proxy_type:?} must not dispatch Sudo::{name}"
                );
            }
            assert!(ProxyType::Any.filter(call));
        }
        for call in &non_fungible_calls {
            let name = call.get_call_metadata().function_name;
            assert!(
                ProxyType::NonFungible.filter(call),
                "NonFungible still dispatches {name}"
            );
        }

        // The advertised allowlist agrees with the executable filter.
        for proxy_type in [ProxyType::NonTransfer, ProxyType::NonFungible] {
            let advertised = allowed_calls(proxy_type);
            for call in value_calls.iter().chain(sudo_calls.iter()) {
                let metadata = call.get_call_metadata();
                let name = format!("{}::{}", metadata.pallet_name, metadata.function_name);
                assert_eq!(
                    advertised.contains(&name),
                    proxy_type.filter(call),
                    "{name}"
                );
            }
        }
    }

    /// Multisig wrappers re-dispatch the inner call on a fresh `Signed(multisig)`
    /// origin that does not carry the proxy filter. Restricted proxies must not
    /// reach them; `Any` still can.
    #[test]
    fn restricted_proxies_cannot_reach_multisig_wrappers() {
        use subtensor_runtime_common::AccountId;

        let other = AccountId::new([3; 32]);
        let inner = RuntimeCall::System(frame_system::Call::remark { remark: vec![] });
        let wrappers = [
            RuntimeCall::Multisig(pallet_multisig::Call::as_multi_threshold_1 {
                other_signatories: vec![other.clone()],
                call: alloc::boxed::Box::new(inner.clone()),
            }),
            RuntimeCall::Multisig(pallet_multisig::Call::as_multi {
                threshold: 2,
                other_signatories: vec![other],
                maybe_timepoint: None,
                call: alloc::boxed::Box::new(inner),
                max_weight: frame_support::weights::Weight::from_parts(1, 1),
            }),
        ];
        for call in &wrappers {
            let name = call.get_call_metadata().function_name;
            for proxy_type in [
                ProxyType::NonTransfer,
                ProxyType::NonFungible,
                ProxyType::NonCritical,
            ] {
                assert!(
                    !proxy_type.filter(call),
                    "{proxy_type:?} must not dispatch Multisig::{name}"
                );
            }
            assert!(ProxyType::Any.filter(call));
        }
    }

    #[test]
    fn non_critical_is_everything_but_sudo_and_critical_ops() {
        let denied = &(&(&group_calls::<SudoCalls>() | &group_calls::<BurnedRegistrationCalls>())
            | &(&group_calls::<RootRegistrationCalls>() | &group_calls::<CriticalNetworkCalls>()))
            | &group_calls::<ColdkeySwapCalls>();
        let denied = &denied | &group_calls::<(CrowdloanCalls, MultisigCalls)>();
        let denied = &denied | &group_calls::<BasketTradingCalls>();
        let denied = &denied | &group_calls::<(MevShieldStoreEncryptedCalls, OwnerKeyCalls)>();
        assert_eq!(
            allowed_calls(ProxyType::NonCritical),
            &all_runtime_calls() - &denied
        );
    }

    #[test]
    fn indirect_value_calls_match_proxy_permissions_and_runtime_api_metadata() {
        use frame_support::weights::Weight;
        use subtensor_runtime_common::{AccountId, TaoBalance};

        let dest = AccountId::new([2; 32]);
        let calls = [
            RuntimeCall::EVM(pallet_evm::Call::call {
                source: Default::default(),
                target: Default::default(),
                input: vec![],
                value: 1.into(),
                gas_limit: 100_000,
                max_fee_per_gas: 1.into(),
                max_priority_fee_per_gas: None,
                nonce: None,
                access_list: vec![],
                authorization_list: Default::default(),
            }),
            RuntimeCall::Contracts(pallet_contracts::Call::call {
                dest: dest.clone().into(),
                value: TaoBalance::from(1),
                gas_limit: Weight::from_parts(100_000, 0),
                storage_deposit_limit: None,
                data: vec![],
            }),
            RuntimeCall::Crowdloan(pallet_crowdloan::Call::create {
                deposit: TaoBalance::from(1),
                min_contribution: TaoBalance::from(1),
                cap: TaoBalance::from(10),
                end: 100,
                call: None,
                target_address: Some(dest),
            }),
            RuntimeCall::Crowdloan(pallet_crowdloan::Call::contribute {
                crowdloan_id: 0,
                amount: TaoBalance::from(1),
            }),
            RuntimeCall::Crowdloan(pallet_crowdloan::Call::finalize { crowdloan_id: 0 }),
        ];

        for proxy_type in [
            ProxyType::Any,
            ProxyType::NonCritical,
            ProxyType::NonTransfer,
            ProxyType::NonFungible,
        ] {
            // This is the metadata provider exposed by ProxyFilterRuntimeApi.
            let infos = get_proxy_filters(Some(vec![proxy_type as u8]));
            assert_eq!(infos.len(), 1);
            let info = infos.first().unwrap();
            assert_eq!(info.proxy_type, proxy_type as u8);
            for call in &calls {
                let metadata = call.get_call_metadata();
                let is_crowdloan = metadata.pallet_name == "Crowdloan";
                // Crowdloan wrappers re-dispatch as the real coldkey; they stay on Any
                // only. EVM / Contracts remain on NonCritical.
                let expected = match proxy_type {
                    ProxyType::Any => true,
                    ProxyType::NonCritical => !is_crowdloan,
                    ProxyType::NonTransfer | ProxyType::NonFungible => false,
                    _ => false,
                };
                let executable = proxy_type.filter(call);
                let advertised = match &info.filter_mode {
                    FilterMode::AllowAll => true,
                    FilterMode::Allow(allowed) => allowed.iter().any(|info| {
                        info.pallet_name == metadata.pallet_name.as_bytes()
                            && info.call_name == metadata.function_name.as_bytes()
                    }),
                };
                assert_eq!(
                    executable, expected,
                    "{proxy_type:?}: {}::{} executable filter",
                    metadata.pallet_name, metadata.function_name,
                );
                assert_eq!(
                    advertised, executable,
                    "{proxy_type:?}: {}::{} runtime API metadata",
                    metadata.pallet_name, metadata.function_name,
                );
            }
        }
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
            ProxyType::SwapHotkey,
            ProxyType::SubnetLeaseBeneficiary,
            ProxyType::RootClaim,
            ProxyType::Validate,
            ProxyType::Weights,
        ]
        .into_iter()
        .collect::<BTreeSet<_>>();

        assert_eq!(actual, expected);
    }

    /// `Weights` sits strictly inside `Validate`; only `Any`, `NonTransfer` and
    /// `Validate` may hand out a `Weights` grant, and nothing narrower than
    /// `NonTransfer` may hand out `Validate`.
    #[test]
    fn validate_and_weights_superset_ordering() {
        let supersets = |child: ProxyType| {
            all_proxy_types()
                .into_iter()
                .filter(|parent| parent.is_superset(&child))
                .collect::<BTreeSet<_>>()
        };
        assert_eq!(
            supersets(ProxyType::Weights),
            [
                ProxyType::Any,
                ProxyType::NonTransfer,
                ProxyType::Validate,
                ProxyType::Weights,
            ]
            .into_iter()
            .collect::<BTreeSet<_>>()
        );
        assert_eq!(
            supersets(ProxyType::Validate),
            [ProxyType::Any, ProxyType::NonTransfer, ProxyType::Validate]
                .into_iter()
                .collect::<BTreeSet<_>>()
        );
        assert!(!ProxyType::Weights.is_superset(&ProxyType::Validate));
        assert!(allowed_calls(ProxyType::Weights).is_subset(&allowed_calls(ProxyType::Validate)));
        assert!(allowed_calls(ProxyType::Validate).len() > allowed_calls(ProxyType::Weights).len());
    }

    #[test]
    fn owner_allows_only_owner_settable_config() {
        let owner = allowed_calls(ProxyType::Owner);
        // Owner-settable subnet params + subnet identity.
        assert!(owner.contains("AdminUtils::sudo_set_serving_rate_limit"));
        assert!(owner.contains("AdminUtils::sudo_set_max_difficulty"));
        assert!(owner.contains("SubtensorModule::set_subnet_identity"));
        // Canonical owner-or-root tempo control lives in AdminUtils; deprecated
        // Subtensor entry points remain available for encoded-call compatibility.
        assert!(owner.contains("AdminUtils::sudo_set_tempo"));
        assert!(owner.contains("AdminUtils::sudo_set_activity_cutoff_factor"));
        assert!(owner.contains("SubtensorModule::set_tempo"));
        assert!(owner.contains("SubtensorModule::set_activity_cutoff_factor"));
        // Root-only admin is not owner-settable (gated by `ensure_root`).
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
                "SubtensorModule::transfer_stake_and_hotkey",
            ])
        );
        assert_eq!(
            allowed_calls(ProxyType::SmallTransfer),
            expected(&[
                "Balances::transfer_keep_alive",
                "Balances::transfer_allow_death",
                "SubtensorModule::transfer_stake",
                "SubtensorModule::transfer_stake_and_hotkey",
            ])
        );
        assert_eq!(
            allowed_calls(ProxyType::Staking),
            expected(&[
                "SubtensorModule::add_collateral",
                "SubtensorModule::add_stake",
                "SubtensorModule::add_stake_limit",
                "SubtensorModule::remove_stake",
                "SubtensorModule::remove_stake_limit",
                "SubtensorModule::remove_stake_full_limit",
                "SubtensorModule::unstake_all",
                "SubtensorModule::unstake_all_alpha",
                "SubtensorModule::move_stake",
                "SubtensorModule::move_stake_limit",
                "SubtensorModule::set_min_collateral",
                "SubtensorModule::stake_into_basket",
                "SubtensorModule::swap_stake",
                "SubtensorModule::swap_stake_limit",
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
            expected(&[
                "SubtensorModule::claim_root",
                "SubtensorModule::claim_root_with_hotkey",
            ])
        );
        assert_eq!(
            allowed_calls(ProxyType::BasketTrading),
            expected(&["SubtensorModule::swap_basket"])
        );
        assert_eq!(
            allowed_calls(ProxyType::SudoUncheckedSetCode),
            expected(&["Sudo::sudo_unchecked_weight"])
        );
        assert_eq!(
            allowed_calls(ProxyType::Weights),
            expected(&[
                "SubtensorModule::batch_commit_weights",
                "SubtensorModule::batch_reveal_weights",
                "SubtensorModule::batch_set_weights",
                "SubtensorModule::commit_crv3_mechanism_weights",
                "SubtensorModule::commit_mechanism_weights",
                "SubtensorModule::commit_timelocked_mechanism_weights",
                "SubtensorModule::commit_timelocked_weights",
                "SubtensorModule::commit_weights",
                "SubtensorModule::reveal_mechanism_weights",
                "SubtensorModule::reveal_weights",
                "SubtensorModule::set_mechanism_weights",
                "SubtensorModule::set_weights",
            ])
        );
        assert_eq!(
            allowed_calls(ProxyType::Validate),
            expected(&[
                "Commitments::set_commitment",
                "SubtensorModule::associate_evm_key",
                "SubtensorModule::batch_commit_weights",
                "SubtensorModule::batch_reveal_weights",
                "SubtensorModule::batch_set_weights",
                "SubtensorModule::commit_crv3_mechanism_weights",
                "SubtensorModule::commit_mechanism_weights",
                "SubtensorModule::commit_timelocked_mechanism_weights",
                "SubtensorModule::commit_timelocked_weights",
                "SubtensorModule::commit_weights",
                "SubtensorModule::reveal_mechanism_weights",
                "SubtensorModule::reveal_weights",
                "SubtensorModule::serve_axon",
                "SubtensorModule::serve_axon_tls",
                "SubtensorModule::serve_prometheus",
                "SubtensorModule::set_mechanism_weights",
                "SubtensorModule::set_weights",
            ])
        );
    }

    /// Per-type deny proof for the two spec-470 hotkey-operations grants. Every
    /// inventory group that moves, locks, burns or spends value, rotates a key,
    /// changes take or child keys, or re-dispatches on a fresh / inherited origin
    /// (`Utility`, `Proxy`, `Multisig`, `Sudo`, `MevShield::store_encrypted`,
    /// EVM, Contracts, Crowdloan) is disjoint from both allow-lists. Because the
    /// inventory groups partition every runtime call, a new value-moving call
    /// that lands in one of these groups is covered without editing this test.
    #[test]
    fn validate_and_weights_reach_no_value_key_or_wrapper_call() {
        let forbidden = group_calls::<(
            BalanceTransferCalls,
            BalanceMaintenanceCalls,
            StakeManagementCalls,
            StakeTransferCalls,
            BurnedRegistrationCalls,
            RootRegistrationCalls,
            FaucetCalls,
            HotkeySwapCalls,
            ColdkeySwapCalls,
            CriticalNetworkCalls,
            ChildKeyCalls,
            RootClaimCalls,
            BasketTradingCalls,
            SubtensorValueCalls,
            OwnerKeyCalls,
        )>();
        let forbidden = &forbidden
            | &group_calls::<(
                SudoCalls,
                MultisigCalls,
                MevShieldStoreEncryptedCalls,
                MevShieldCalls,
                EvmCalls,
                EthereumCalls,
                ContractsCalls,
                CrowdloanCalls,
                UtilityCalls,
                ProxyCalls,
                SchedulerCalls,
                PreimageCalls,
                SwapCalls,
                LimitOrdersCalls,
                SubnetManagementCalls,
                RootConfigCalls,
            )>();
        // `SubtensorCommonCalls` still holds take changes, identity, and the
        // coldkey-side association calls; none of them belong to a hotkey grant.
        let forbidden = &forbidden | &group_calls::<SubtensorCommonCalls>();

        for proxy_type in [ProxyType::Validate, ProxyType::Weights] {
            let leaked = allowed_calls(proxy_type)
                .intersection(&forbidden)
                .cloned()
                .collect::<Vec<_>>();
            assert!(
                leaked.is_empty(),
                "{proxy_type:?} reaches forbidden calls: {leaked:?}"
            );
        }
    }

    /// `Validate` reaches `Commitments::set_commitment`, which reserves
    /// `InitialDeposit + fields * FieldDeposit` from the real. The "no value
    /// movement" claim for `Validate` holds only while both are zero, so raising
    /// either constant must force this grant to be re-evaluated.
    #[test]
    fn validate_commitment_grant_relies_on_zero_commitment_deposit() {
        use subtensor_runtime_common::{TaoBalance, Token};

        assert_eq!(crate::CommitmentInitialDeposit::get(), TaoBalance::ZERO);
        assert_eq!(crate::CommitmentFieldDeposit::get(), TaoBalance::ZERO);
    }

    /// Executable-filter proof for `Validate` / `Weights`: value-moving, key, and
    /// wrapper calls are refused at dispatch (not only in the advertised
    /// metadata); `Weights` also refuses the serving / association / commitment
    /// calls that `Validate` admits; and the advertised allow-lists agree with the
    /// filter for every call checked.
    #[test]
    fn validate_and_weights_executable_filter_denies_value_moves() {
        use alloc::boxed::Box;
        use frame_support::weights::Weight;
        use frame_system::Call as SystemCall;
        use pallet_balances::Call as BalancesCall;
        use pallet_subtensor::Call as SubtensorCall;
        use pallet_subtensor_proxy::Call as ProxyCall;
        use pallet_subtensor_utility::Call as UtilityCall;
        use subtensor_runtime_common::{AccountId, AlphaBalance, NetUid, TaoBalance};

        let hotkey = AccountId::new([7u8; 32]);
        let other = AccountId::new([3u8; 32]);
        let netuid = NetUid::from(1);
        let set_weights = RuntimeCall::SubtensorModule(SubtensorCall::set_weights {
            netuid,
            dests: vec![0],
            weights: vec![1],
            version_key: 0,
        });

        let value_or_wrapper_calls = [
            RuntimeCall::Balances(BalancesCall::transfer_keep_alive {
                dest: other.clone().into(),
                value: TaoBalance::from(1),
            }),
            RuntimeCall::Balances(BalancesCall::transfer_all {
                dest: other.clone().into(),
                keep_alive: false,
            }),
            RuntimeCall::SubtensorModule(SubtensorCall::add_stake {
                hotkey: hotkey.clone(),
                netuid,
                amount_staked: TaoBalance::from(1),
            }),
            RuntimeCall::SubtensorModule(SubtensorCall::remove_stake {
                hotkey: hotkey.clone(),
                netuid,
                amount_unstaked: AlphaBalance::from(1),
            }),
            RuntimeCall::SubtensorModule(SubtensorCall::unstake_all {
                hotkey: hotkey.clone(),
            }),
            RuntimeCall::SubtensorModule(SubtensorCall::transfer_stake {
                destination_coldkey: other.clone(),
                hotkey: hotkey.clone(),
                origin_netuid: netuid,
                destination_netuid: netuid,
                alpha_amount: AlphaBalance::from(1),
            }),
            RuntimeCall::SubtensorModule(SubtensorCall::move_stake {
                origin_hotkey: hotkey.clone(),
                destination_hotkey: other.clone(),
                origin_netuid: netuid,
                destination_netuid: netuid,
                alpha_amount: AlphaBalance::from(1),
            }),
            RuntimeCall::SubtensorModule(SubtensorCall::swap_basket {
                hotkey: hotkey.clone(),
                origin_netuid: netuid,
                destination_netuid: NetUid::from(2),
                amount: AlphaBalance::from(1),
                min_amount_out: 0,
            }),
            RuntimeCall::SubtensorModule(SubtensorCall::stake_into_basket {
                hotkey: hotkey.clone(),
                amount_staked: TaoBalance::from(1),
            }),
            RuntimeCall::SubtensorModule(SubtensorCall::claim_root_with_hotkey {
                hotkey: hotkey.clone(),
            }),
            RuntimeCall::SubtensorModule(SubtensorCall::burned_register {
                netuid,
                hotkey: hotkey.clone(),
            }),
            RuntimeCall::SubtensorModule(SubtensorCall::root_register {
                hotkey: hotkey.clone(),
            }),
            RuntimeCall::SubtensorModule(SubtensorCall::swap_hotkey {
                hotkey: hotkey.clone(),
                new_hotkey: other.clone(),
                netuid: None,
            }),
            RuntimeCall::SubtensorModule(SubtensorCall::set_children {
                hotkey: hotkey.clone(),
                netuid,
                children: vec![],
            }),
            RuntimeCall::SubtensorModule(SubtensorCall::increase_take {
                hotkey: hotkey.clone(),
                take: sp_runtime::PerU16::from_percent(1),
            }),
            RuntimeCall::SubtensorModule(SubtensorCall::burn_alpha {
                hotkey: hotkey.clone(),
                amount: AlphaBalance::from(1),
                netuid,
            }),
            RuntimeCall::Utility(UtilityCall::batch {
                calls: vec![set_weights.clone()],
            }),
            RuntimeCall::Utility(UtilityCall::batch_all {
                calls: vec![set_weights.clone()],
            }),
            RuntimeCall::Utility(UtilityCall::with_weight {
                call: Box::new(set_weights.clone()),
                weight: Weight::zero(),
            }),
            RuntimeCall::Proxy(ProxyCall::add_proxy {
                delegate: other.clone().into(),
                proxy_type: ProxyType::Weights,
                delay: 0,
            }),
            RuntimeCall::Proxy(ProxyCall::set_real_pays_fee {
                delegate: other.clone().into(),
                pays_fee: true,
            }),
            RuntimeCall::Multisig(pallet_multisig::Call::as_multi_threshold_1 {
                other_signatories: vec![other.clone()],
                call: Box::new(set_weights.clone()),
            }),
            RuntimeCall::Sudo(pallet_sudo::Call::sudo {
                call: Box::new(set_weights.clone()),
            }),
            RuntimeCall::MevShield(pallet_shield::Call::store_encrypted {
                encrypted_call: Default::default(),
            }),
            RuntimeCall::EVM(pallet_evm::Call::call {
                source: Default::default(),
                target: Default::default(),
                input: vec![],
                value: 0.into(),
                gas_limit: 100_000,
                max_fee_per_gas: 1.into(),
                max_priority_fee_per_gas: None,
                nonce: None,
                access_list: vec![],
                authorization_list: Default::default(),
            }),
            RuntimeCall::System(SystemCall::remark { remark: vec![] }),
        ];
        let validate_only_calls = [
            RuntimeCall::SubtensorModule(SubtensorCall::serve_axon {
                netuid,
                version: 1,
                ip: 1,
                port: 1,
                ip_type: 4,
                protocol: 0,
                placeholder1: 0,
                placeholder2: 0,
            }),
            RuntimeCall::SubtensorModule(SubtensorCall::serve_prometheus {
                netuid,
                version: 1,
                ip: 1,
                port: 1,
                ip_type: 4,
            }),
            RuntimeCall::SubtensorModule(SubtensorCall::associate_evm_key {
                netuid,
                evm_key: Default::default(),
                block_number: 0,
                signature: sp_core::ecdsa::Signature::from_raw([0u8; 65]),
            }),
            RuntimeCall::Commitments(pallet_commitments::Call::set_commitment {
                netuid,
                info: Box::default(),
            }),
        ];
        let weight_calls = [
            set_weights.clone(),
            RuntimeCall::SubtensorModule(SubtensorCall::commit_weights {
                netuid,
                commit_hash: Default::default(),
            }),
            RuntimeCall::SubtensorModule(SubtensorCall::reveal_weights {
                netuid,
                uids: vec![0],
                values: vec![1],
                salt: vec![],
                version_key: 0,
            }),
            RuntimeCall::SubtensorModule(SubtensorCall::batch_set_weights {
                netuids: vec![codec::Compact(netuid)],
                weights: vec![vec![(codec::Compact(0u16), codec::Compact(1u16))]],
                version_keys: vec![codec::Compact(0u64)],
            }),
        ];

        let advertised = |proxy_type: ProxyType, call: &RuntimeCall| {
            let metadata = call.get_call_metadata();
            allowed_calls(proxy_type).contains(&format!(
                "{}::{}",
                metadata.pallet_name, metadata.function_name
            ))
        };

        for call in &value_or_wrapper_calls {
            let name = call.get_call_metadata().function_name;
            for proxy_type in [ProxyType::Validate, ProxyType::Weights] {
                assert!(
                    !proxy_type.filter(call),
                    "{proxy_type:?} must not dispatch {name}"
                );
                assert!(
                    !advertised(proxy_type, call),
                    "{proxy_type:?} advertises {name}"
                );
            }
        }
        for call in &validate_only_calls {
            let name = call.get_call_metadata().function_name;
            assert!(
                ProxyType::Validate.filter(call),
                "Validate dispatches {name}"
            );
            assert!(
                !ProxyType::Weights.filter(call),
                "Weights must not dispatch {name}"
            );
            assert!(advertised(ProxyType::Validate, call));
            assert!(!advertised(ProxyType::Weights, call));
        }
        for call in &weight_calls {
            let name = call.get_call_metadata().function_name;
            for proxy_type in [ProxyType::Validate, ProxyType::Weights] {
                assert!(proxy_type.filter(call), "{proxy_type:?} dispatches {name}");
                assert!(advertised(proxy_type, call));
            }
        }
        // The broad grants that already covered these hotkey operations keep them.
        for call in weight_calls.iter().chain(validate_only_calls.iter()) {
            let name = call.get_call_metadata().function_name;
            for proxy_type in [
                ProxyType::NonTransfer,
                ProxyType::NonFungible,
                ProxyType::NonCritical,
            ] {
                assert!(proxy_type.filter(call), "{proxy_type:?} keeps {name}");
            }
        }
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
            assert!(!allowed.contains("Multisig::as_multi_threshold_1"));
            assert!(!allowed.contains("Multisig::as_multi"));
            assert!(!allowed.contains("Crowdloan::finalize"));
            assert!(!allowed.contains("Sudo::sudo"));
            assert!(!allowed.contains("MevShield::store_encrypted"));
            assert!(!allowed.contains("AdminUtils::sudo_set_sn_owner_hotkey"));
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

    /// `BasketTrading` is a single-call grant: it admits `swap_basket` and nothing else,
    /// no other narrow proxy admits `swap_basket`, and the broad proxies include it only
    /// where value-moving stake calls are allowed.
    #[test]
    fn basket_trading_proxy_grants_exactly_swap_basket() {
        use frame_system::Call as SystemCall;
        use pallet_subtensor::Call as SubtensorCall;
        use subtensor_runtime_common::{AccountId, AlphaBalance, NetUid, TaoBalance};

        let hotkey = AccountId::new([7u8; 32]);
        let swap_basket = RuntimeCall::SubtensorModule(SubtensorCall::swap_basket {
            hotkey: hotkey.clone(),
            origin_netuid: NetUid::from(1),
            destination_netuid: NetUid::from(2),
            amount: AlphaBalance::from(1),
            min_amount_out: 0,
        });
        let stake_into_basket = RuntimeCall::SubtensorModule(SubtensorCall::stake_into_basket {
            hotkey: hotkey.clone(),
            amount_staked: TaoBalance::from(1),
        });
        let claim = RuntimeCall::SubtensorModule(SubtensorCall::claim_root_with_hotkey { hotkey });
        let remark = RuntimeCall::System(SystemCall::remark { remark: vec![] });

        // The trading proxy admits the trade and nothing adjacent to it.
        assert!(proxy_type_filter(&ProxyType::BasketTrading, &swap_basket));
        for denied in [&stake_into_basket, &claim, &remark] {
            assert!(!proxy_type_filter(&ProxyType::BasketTrading, denied));
        }

        // No other narrow proxy can be used to trade the basket.
        for narrow in [
            ProxyType::Owner,
            ProxyType::Staking,
            ProxyType::Registration,
            ProxyType::Transfer,
            ProxyType::SmallTransfer,
            ProxyType::RootClaim,
            ProxyType::ChildKeys,
            ProxyType::SwapHotkey,
            ProxyType::SubnetLeaseBeneficiary,
            ProxyType::SudoUncheckedSetCode,
            ProxyType::RootWeights,
            ProxyType::Validate,
            ProxyType::Weights,
        ] {
            assert!(
                !proxy_type_filter(&narrow, &swap_basket),
                "{narrow:?} must not admit swap_basket"
            );
        }

        // Only `Any` may trade among the broad proxies; `NonFungible` (no value movement)
        // may not. `NonTransfer` / `NonCritical` are pinned separately below.
        assert!(proxy_type_filter(&ProxyType::Any, &swap_basket));
        assert!(!proxy_type_filter(&ProxyType::NonFungible, &swap_basket));

        // Superset relation: only `Any` covers the trading grant.
        let supersets = all_proxy_types()
            .into_iter()
            .filter(|proxy_type| proxy_type.is_superset(&ProxyType::BasketTrading))
            .collect::<BTreeSet<_>>();
        assert_eq!(
            supersets,
            [ProxyType::Any, ProxyType::BasketTrading]
                .into_iter()
                .collect::<BTreeSet<_>>()
        );
    }

    /// `swap_basket` needs the explicit `BasketTrading` grant: the broad `NonTransfer`
    /// and `NonCritical` delegations do not admit it (PR #3150 calibration pass §5.6),
    /// so an existing delegate does not gain trading power at upgrade without opting in.
    #[test]
    fn broad_proxies_do_not_admit_swap_basket_without_explicit_grant() {
        use pallet_subtensor::Call as SubtensorCall;
        use subtensor_runtime_common::{AccountId, AlphaBalance, NetUid};

        let swap_basket = RuntimeCall::SubtensorModule(SubtensorCall::swap_basket {
            hotkey: AccountId::new([7u8; 32]),
            origin_netuid: NetUid::from(1),
            destination_netuid: NetUid::from(2),
            amount: AlphaBalance::from(1),
            min_amount_out: 0,
        });
        for broad in [ProxyType::NonTransfer, ProxyType::NonCritical] {
            assert!(
                !proxy_type_filter(&broad, &swap_basket),
                "{broad:?} must not admit swap_basket"
            );
            assert!(!allowed_calls(broad).contains("SubtensorModule::swap_basket"));
        }
        assert!(proxy_type_filter(&ProxyType::BasketTrading, &swap_basket));
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
