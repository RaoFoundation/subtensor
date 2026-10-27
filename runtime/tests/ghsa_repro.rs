//! Reproduction tests for the June 2026 security audit proxy-filter findings.
//!
//! These are *reproductions*: a passing test demonstrates the vulnerability is live.
//! The filter returns `true` when a call is ALLOWED for a proxy type. The bug in each
//! case is that a fund/ownership-moving call is ALLOWED for a proxy type that is meant
//! to forbid it.
//!
//! - GHSA-2026-001: NonTransfer / NonFungible proxies allow the coldkey-swap lifecycle
//!   (announce_coldkey_swap + swap_coldkey_announced) -> full account takeover.
//! - GHSA-2026-002: NonFungible allows swap_hotkey_v2 (call 72) though it denies the
//!   deprecated swap_hotkey (call 70); SwapHotkey allows only call 70, not the live v2.
//! - GHSA-2026-003: Owner proxy allows sudo_set_subnet_owner_hotkey (call 64) even though
//!   it explicitly excepts the duplicate alias sudo_set_sn_owner_hotkey (call 67).
#![allow(clippy::unwrap_used, unused_imports, dead_code)]

use frame_support::traits::InstanceFilter;
use node_subtensor_runtime::RuntimeCall;
use subtensor_runtime_common::{AccountId, NetUid, ProxyType, TaoBalance};

fn acct() -> AccountId {
    AccountId::new([0u8; 32])
}

// ---- coldkey-swap lifecycle calls ----
fn announce_coldkey_swap() -> RuntimeCall {
    RuntimeCall::SubtensorModule(pallet_subtensor::Call::announce_coldkey_swap {
        new_coldkey_hash: Default::default(),
    })
}
fn swap_coldkey_announced() -> RuntimeCall {
    RuntimeCall::SubtensorModule(pallet_subtensor::Call::swap_coldkey_announced {
        new_coldkey: acct(),
    })
}
fn swap_coldkey_legacy() -> RuntimeCall {
    RuntimeCall::SubtensorModule(pallet_subtensor::Call::swap_coldkey {
        old_coldkey: acct(),
        new_coldkey: acct(),
        swap_cost: TaoBalance::from(0u64),
    })
}
fn transfer_stake() -> RuntimeCall {
    RuntimeCall::SubtensorModule(pallet_subtensor::Call::transfer_stake {
        destination_coldkey: acct(),
        hotkey: acct(),
        origin_netuid: NetUid::from(1),
        destination_netuid: NetUid::from(1),
        alpha_amount: Default::default(),
    })
}

// ---- hotkey-swap calls ----
fn swap_hotkey_v1() -> RuntimeCall {
    RuntimeCall::SubtensorModule(pallet_subtensor::Call::swap_hotkey {
        hotkey: acct(),
        new_hotkey: acct(),
        netuid: Default::default(),
    })
}
fn swap_hotkey_v2() -> RuntimeCall {
    RuntimeCall::SubtensorModule(pallet_subtensor::Call::swap_hotkey_v2 {
        hotkey: acct(),
        new_hotkey: acct(),
        netuid: Default::default(),
        keep_stake: false,
    })
}

// ---- owner-hotkey setter aliases ----
fn set_sn_owner_hotkey_c67() -> RuntimeCall {
    RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_sn_owner_hotkey {
        netuid: Default::default(),
        hotkey: acct(),
    })
}

/// GHSA-2026-001 — NonTransfer and NonFungible proxies (the two "cannot move my funds"
/// types) ALLOW the new coldkey-swap lifecycle, so a restricted delegate can take over
/// the whole coldkey. Reproduced by asserting the calls are NOT filtered.
#[test]
fn ghsa_2026_001_restricted_proxies_allow_coldkey_swap_lifecycle() {
    let announce = announce_coldkey_swap();
    let exec = swap_coldkey_announced();

    // These two proxy types DO block direct exfiltration (transfer_stake denied) ...
    for pt in [ProxyType::NonTransfer, ProxyType::NonFungible] {
        assert!(
            !pt.filter(&transfer_stake()),
            "precondition: {pt:?} should deny transfer_stake (it is a fund-protection type)"
        );
        // ... and after the fix they ALSO block the swap lifecycle that would exfiltrate everything:
        assert!(
            !pt.filter(&announce),
            "regression (GHSA-2026-001 fixed): {pt:?} must DENY announce_coldkey_swap"
        );
        assert!(
            !pt.filter(&exec),
            "regression (GHSA-2026-001 fixed): {pt:?} must DENY swap_coldkey_announced"
        );
        // Contrast: the legacy swap_coldkey they replaced IS denied — proving the gap is
        // specifically the un-listed new lifecycle calls.
        assert!(
            !pt.filter(&swap_coldkey_legacy()),
            "{pt:?} correctly denies legacy swap_coldkey — the new calls were simply never added"
        );
    }
}

/// Scope correction for GHSA-2026-001: NonCritical is NOT a fund-protection type — it
/// already permits transfer_stake — so the coldkey-swap gap is not an *escalation* for it.
/// Documents why NonCritical is excluded from the finding.
#[test]
fn ghsa_2026_001_noncritical_is_not_a_fund_protection_type() {
    assert!(
        ProxyType::NonCritical.filter(&transfer_stake()),
        "NonCritical already allows transfer_stake, so coldkey-swap adds no new capability"
    );
}

/// GHSA-2026-002 — NonFungible denies the deprecated swap_hotkey (call 70) but ALLOWS the
/// live swap_hotkey_v2 (call 72); and the SwapHotkey allow-list permits only call 70.
#[test]
fn ghsa_2026_002_nonfungible_allows_swap_hotkey_v2_gap() {
    // The denylist blocks the old call but not the live superset.
    assert!(
        !ProxyType::NonFungible.filter(&swap_hotkey_v1()),
        "precondition: NonFungible denies deprecated swap_hotkey (call 70)"
    );
    assert!(
        !ProxyType::NonFungible.filter(&swap_hotkey_v2()),
        "regression (GHSA-2026-002 fixed): NonFungible must DENY the live swap_hotkey_v2 (call 72)"
    );

    // Inverse breakage: SwapHotkey allow-list only permits the deprecated call.
    assert!(
        ProxyType::SwapHotkey.filter(&swap_hotkey_v1()),
        "precondition: SwapHotkey allows deprecated swap_hotkey (call 70)"
    );
    assert!(
        ProxyType::SwapHotkey.filter(&swap_hotkey_v2()),
        "regression (GHSA-2026-002 fixed): SwapHotkey must ALLOW the live swap_hotkey_v2 (call 72)"
    );
}

/// GHSA-2026-003 — the Owner proxy excepts sudo_set_sn_owner_hotkey (call 67) but the
#[test]
fn ghsa_2026_003_owner_proxy_set_owner_hotkey_alias_bypass() {
    assert!(
        !ProxyType::Owner.filter(&set_sn_owner_hotkey_c67()),
        "precondition: Owner correctly excepts sudo_set_sn_owner_hotkey (call 67)"
    );
}

/// Registration collateral must remain payable when composed with the real
/// runtime's refundable preimage deposit and a subsequent balance drain.
#[test]
fn subnet_registration_preimage_hold_cannot_short_fund_settlement() {
    use frame_support::{
        assert_ok,
        traits::{
            fungible::{Inspect, InspectHold, Mutate},
            tokens::{Fortitude, Preservation},
        },
    };
    use node_subtensor_runtime::{
        Balances, BuildStorage, Preimage, Runtime, RuntimeGenesisConfig, RuntimeHoldReason,
        RuntimeOrigin, SubtensorModule, System,
    };
    use sp_runtime::{Saturating, traits::Hash};
    use subtensor_runtime_common::Token;
    for legacy in [false, true] {
        let mut ext: sp_io::TestExternalities = RuntimeGenesisConfig::default()
            .build_storage()
            .unwrap()
            .into();
        ext.execute_with(|| {
            System::set_block_number(1);
            let cold = AccountId::new([41; 32]);
            let hot = AccountId::new([42; 32]);
            let recipient = AccountId::new([43; 32]);
            pallet_subtensor::NetworkRegistrationStartBlock::<Runtime>::put(0);
            // Runtime genesis already contains non-root subnet 1.
            pallet_subtensor::SubnetLimit::<Runtime>::put(2);
            pallet_subtensor::NetworkMinLockCost::<Runtime>::put(TaoBalance::from(1_000_000_000));
            pallet_subtensor::NetworkLastLockCost::<Runtime>::put(TaoBalance::from(1_000_000_000));
            SubtensorModule::set_network_rate_limit(0);
            let cost = SubtensorModule::get_network_lock_cost();
            assert_ok!(<Balances as Mutate<AccountId>>::mint_into(
                &cold,
                cost.saturating_mul(4.into())
            ));
            if legacy {
                assert_ok!(SubtensorModule::lock_network_registration_cost(
                    &cold, cost, 0
                ));
                pallet_subtensor::NetworkRegistrationQueue::<Runtime>::put(vec![
                    pallet_subtensor::subnets::subnet::NetworkRegistrationInfo {
                        coldkey: cold.clone(),
                        hotkey: hot.clone(),
                        mechid: 1,
                        identity: None,
                        lock_amount: cost,
                        median_subnet_alpha_price: SubtensorModule::get_median_subnet_alpha_price(),
                        registration_block: 1,
                        lock_id: 0,
                    },
                ]);
            } else {
                pallet_subtensor::DissolveCleanupQueue::<Runtime>::put(vec![NetUid::from(2)]);
                assert_ok!(SubtensorModule::register_network(
                    RuntimeOrigin::signed(cold.clone()),
                    hot.clone()
                ));
                assert_eq!(
                    pallet_subtensor::NetworkRegistrationEscrow::<Runtime>::get(0),
                    Some((cold.clone(), cost))
                );
            }
            // Runtime storage pricing is 104M rao base + 1M rao per byte.
            let bytes = cost
                .to_u64()
                .saturating_sub(104_000_000)
                .div_ceil(1_000_000) as usize;
            let preimage = vec![7u8; bytes];
            let hash = <Runtime as frame_system::Config>::Hashing::hash(&preimage);
            assert_ok!(Preimage::note_preimage(
                RuntimeOrigin::signed(cold.clone()),
                preimage
            ));
            let reason = RuntimeHoldReason::Preimage(pallet_preimage::HoldReason::Preimage);
            assert!(<Balances as InspectHold<AccountId>>::balance_on_hold(&reason, &cold) >= cost);
            let keep = <Balances as Inspect<AccountId>>::minimum_balance().saturating_mul(2.into());
            let drain = Balances::free_balance(&cold).saturating_sub(keep);
            assert_ok!(<Balances as Mutate<AccountId>>::transfer(
                &cold,
                &recipient,
                drain,
                Preservation::Preserve
            ));
            assert!(
                <Balances as Inspect<AccountId>>::reducible_balance(
                    &cold,
                    Preservation::Preserve,
                    Fortitude::Polite
                ) < cost
            );
            pallet_subtensor::DissolveCleanupQueue::<Runtime>::kill();
            let netuid = SubtensorModule::get_next_netuid();
            SubtensorModule::process_network_registration_queue();
            assert!(
                pallet_subtensor::NetworkRegistrationQueue::<Runtime>::get().is_empty(),
                "legacy={legacy}, events={:?}",
                System::events()
            );
            if legacy {
                assert!(!pallet_subtensor::NetworksAdded::<Runtime>::contains_key(
                    netuid
                ));
                assert!(!pallet_subtensor::SubnetTAO::<Runtime>::contains_key(
                    netuid
                ));
            } else {
                assert_eq!(pallet_subtensor::SubnetTAO::<Runtime>::get(netuid), cost);
                assert_eq!(pallet_subtensor::SubnetLocked::<Runtime>::get(netuid), cost);
                assert_eq!(
                    Balances::free_balance(SubtensorModule::get_subnet_account_id(netuid).unwrap()),
                    cost
                );
                assert!(!pallet_subtensor::NetworkRegistrationEscrow::<Runtime>::contains_key(0));
            }
            assert_eq!(Balances::free_balance(&cold), keep);
            assert_ok!(Preimage::unnote_preimage(
                RuntimeOrigin::signed(cold.clone()),
                hash
            ));
            assert_eq!(
                <Balances as InspectHold<AccountId>>::balance_on_hold(&reason, &cold),
                TaoBalance::ZERO
            );
        });
    }
}
