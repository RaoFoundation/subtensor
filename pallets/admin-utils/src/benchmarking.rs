//! Benchmarking setup
#![cfg(feature = "runtime-benchmarks")]
#![allow(clippy::arithmetic_side_effects)]
#![allow(clippy::unwrap_used)]

extern crate alloc;
use alloc::vec::Vec;

#[allow(unused)]
use crate::Pallet as AdminUtils;
use frame_benchmarking::v1::account;
use frame_benchmarking::v2::*;
use frame_support::{BoundedVec, assert_noop, dispatch::UnfilteredDispatchable};
use frame_system::RawOrigin;
use pallet_subtensor::SubnetworkN;
use scale_info::prelude::vec;
use sp_runtime::traits::Get;

use super::*;

#[benchmarks]
mod benchmarks {
    use super::*;
    #[cfg(test)]
    use crate::tests::mock;
    use pallet_subtensor::staking::lock::LockState;
    use sp_runtime::PerU16;
    use substrate_fixed::types::{I64F64, U64F64};
    use subtensor_runtime_common::{NetUid, TaoBalance};

    #[benchmark]
    fn swap_authorities(a: Linear<0, 32>) {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);

        let mut value: BoundedVec<
            <T as pallet::Config>::AuthorityId,
            <T as pallet::Config>::MaxAuthorities,
        > = BoundedVec::new();

        for idx in 1..=a {
            let authority: <T as pallet::Config>::AuthorityId = account("Authority", idx, 0u32);
            let result = value.try_push(authority.clone());
            if result.is_err() {
                // Handle the error, perhaps by breaking the loop or logging an error message
            }
        }

        #[extrinsic_call]
        _(RawOrigin::Root, value);
    }

    #[benchmark]
    fn schedule_grandpa_change(a: Linear<0, 32>) {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        let next_authorities = (1..=a)
            .map(|idx| account("Authority", idx, 0u32))
            .collect::<Vec<(sp_consensus_grandpa::AuthorityId, u64)>>();
        let in_blocks = BlockNumberFor::<T>::from(42u32);

        #[extrinsic_call]
        _(RawOrigin::Root, next_authorities, in_blocks, None);
    }

    #[benchmark]
    fn sudo_set_default_take() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        #[extrinsic_call]
        _(RawOrigin::Root, PerU16::from_parts(100)/*default_take*/)/*sudo_set_default_take*/;
    }

    #[benchmark]
    fn sudo_set_serving_rate_limit() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 100u64/*serving_rate_limit*/)/*sudo_set_serving_rate_limit*/;
    }

    #[benchmark]
    fn sudo_set_max_difficulty() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 10000u64/*max_difficulty*/)/*sudo_set_max_difficulty*/;
    }

    #[benchmark]
    fn sudo_set_min_difficulty() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 1000u64/*min_difficulty*/)/*sudo_set_min_difficulty*/;
    }

    #[benchmark]
    fn sudo_set_weights_set_rate_limit() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 3u64/*rate_limit*/)/*sudo_set_weights_set_rate_limit*/;
    }

    #[benchmark]
    fn sudo_set_weights_version_key() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 1u64/*version_key*/)/*sudo_set_weights_version_key*/;
    }

    #[benchmark]
    fn sudo_set_bonds_moving_average() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 100u64/*bonds_moving_average*/)/*sudo_set_bonds_moving_average*/;
    }

    #[benchmark]
    fn sudo_set_bonds_penalty() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*tempo*/
        );

        #[extrinsic_call]
        _(RawOrigin::Root, 1u16.into()/*netuid*/, 100u16/*bonds_penalty*/)/*sudo_set_bonds_penalty*/;
    }

    #[benchmark]
    fn sudo_set_max_allowed_validators() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 10u16/*max_allowed_validators*/)/*sudo_set_max_allowed_validators*/;
    }

    #[benchmark]
    fn sudo_set_difficulty() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 1200000u64/*difficulty*/)/*sudo_set_difficulty*/;
    }

    #[benchmark]
    fn sudo_set_adjustment_interval() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 12u16/*adjustment_interval*/)/*sudo_set_adjustment_interval*/;
    }

    #[benchmark]
    fn sudo_set_target_registrations_per_interval() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 300u16/*target_registrations*/)/*sudo_set_target_registrations_per_interval*/;
    }

    #[benchmark]
    fn sudo_set_activity_cutoff() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 361u16/*activity_cutoff*/)/*sudo_set_activity_cutoff*/;
    }

    #[benchmark]
    fn sudo_set_activity_cutoff_factor() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 5_000u32/*factor_milli*/)/*sudo_set_activity_cutoff_factor*/;
    }

    #[benchmark]
    fn sudo_set_rho() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 300u16/*rho*/)/*sudo_set_rho*/;
    }

    #[benchmark]
    fn sudo_set_kappa() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*sudo_tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 3u16/*kappa*/)/*set_kappa*/;
    }

    #[benchmark]
    fn sudo_set_min_allowed_uids() {
        let netuid = NetUid::from(1);
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(netuid, 1u16 /*tempo*/);

        // Artificially set that some neurons are already registered
        SubnetworkN::<T>::set(netuid, 32);

        #[extrinsic_call]
		_(RawOrigin::Root, netuid, 16u16/*min_allowed_uids*/)/*sudo_set_min_allowed_uids*/;
    }

    #[benchmark]
    fn sudo_set_max_allowed_uids() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 256u16/*max_allowed_uids*/)/*sudo_set_max_allowed_uids*/;
    }

    #[benchmark]
    fn sudo_set_min_allowed_weights() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 10u16/*max_allowed_uids*/)/*sudo_set_min_allowed_weights*/;
    }

    #[benchmark]
    fn sudo_set_immunity_period() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 100u16/*immunity_period*/)/*sudo_set_immunity_period*/;
    }

    #[benchmark]
    fn sudo_set_max_registrations_per_block() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 100u16/*max_registrations*/)/*sudo_set_max_registrations_per_block*/;
    }

    #[benchmark]
    fn sudo_set_max_burn() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 2_000_000_000.into()/*max_burn*/)/*sudo_set_max_burn*/;
    }

    #[benchmark]
    fn sudo_set_min_burn() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 10.into()/*min_burn*/)/*sudo_set_min_burn*/;
    }

    #[benchmark]
    fn sudo_set_network_registration_allowed() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, true/*registration_allowed*/)/*sudo_set_network_registration_allowed*/;
    }

    #[benchmark]
    fn sudo_set_tempo() {
        let netuid = NetUid::from(1);
        let owner: T::AccountId = account("owner", 0, 0);

        // Benchmark the heavier owner path (bounds check + rate-limit read/write),
        // not the root path which bypasses both.
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(netuid, pallet_subtensor::MIN_TEMPO);
        pallet_subtensor::SubnetOwner::<T>::insert(netuid, &owner);

        #[extrinsic_call]
        _(
            RawOrigin::Signed(owner),
            netuid,
            pallet_subtensor::MIN_TEMPO + 1,
        );
    }

    #[benchmark]
    fn sudo_set_commit_reveal_weights_interval() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*sudo_tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 3u64/*interval*/)/*sudo_set_commit_reveal_weights_interval()*/;
    }

    #[benchmark]
    fn sudo_set_commit_reveal_weights_enabled() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*sudo_tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, true/*enabled*/)/*set_commit_reveal_weights_enabled*/;
    }

    #[benchmark]
    fn sudo_set_commit_reveal_version() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*sudo_tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 5u16/*version*/)/*sudo_set_commit_reveal_version()*/;
    }

    #[benchmark]
    fn sudo_set_tx_rate_limit() {
        #[extrinsic_call]
        _(RawOrigin::Root, 100u64);
    }

    #[benchmark]
    fn sudo_set_total_issuance() {
        let call = Call::<T>::sudo_set_total_issuance {
            total_issuance: 100u64.into(),
        };

        #[block]
        {
            assert_noop!(
                call.dispatch_bypass_filter(RawOrigin::Root.into()),
                Error::<T>::Deprecated
            );
        }
    }

    #[benchmark]
    fn sudo_set_rao_recycled() {
        let netuid = NetUid::from(1);
        pallet_subtensor::Pallet::<T>::init_new_network(
            netuid, 1u16, // tempo
        );

        #[extrinsic_call]
        _(RawOrigin::Root, netuid, 100u64.into());
    }

    #[benchmark]
    fn sudo_set_stake_threshold() {
        #[extrinsic_call]
        _(RawOrigin::Root, 100u64);
    }

    #[benchmark]
    fn sudo_set_nominator_min_required_stake() {
        #[extrinsic_call]
        _(RawOrigin::Root, 100u64);
    }

    #[benchmark]
    fn sudo_set_tx_delegate_take_rate_limit() {
        #[extrinsic_call]
        _(RawOrigin::Root, 100u64);
    }

    #[benchmark]
    fn sudo_set_min_delegate_take() {
        #[extrinsic_call]
        _(RawOrigin::Root, PerU16::from_parts(100));
    }

    #[benchmark]
    fn sudo_set_min_childkey_take_per_subnet() {
        let netuid = NetUid::from(1);
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            netuid, 1u16, // tempo
        );
        let take = PerU16::from_parts(pallet_subtensor::Pallet::<T>::get_max_childkey_take() / 2);

        #[extrinsic_call]
        _(RawOrigin::Root, netuid, take);
    }

    #[benchmark]
    fn sudo_set_liquid_alpha_enabled() {
        let netuid = NetUid::from(1);
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            netuid, 1u16, // tempo
        );

        #[extrinsic_call]
        _(RawOrigin::Root, netuid, true);
    }

    #[benchmark]
    fn sudo_set_alpha_values() {
        let netuid = NetUid::from(1);
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            netuid, 1u16, // tempo
        );
        pallet_subtensor::Pallet::<T>::set_liquid_alpha_enabled(netuid, true);

        #[extrinsic_call]
        _(RawOrigin::Root, netuid, 2000u16, 3000u16);
    }

    #[benchmark]
    fn sudo_set_coldkey_swap_announcement_delay() {
        #[extrinsic_call]
        _(RawOrigin::Root, 100u32.into());
    }

    #[benchmark]
    fn sudo_set_coldkey_swap_reannouncement_delay() {
        #[extrinsic_call]
        _(RawOrigin::Root, 100u32.into());
    }

    #[benchmark]
    fn sudo_set_dissolve_network_schedule_duration() {
        #[extrinsic_call]
        _(RawOrigin::Root, 100u32.into());
    }

    #[benchmark]
    fn sudo_set_toggle_transfer() {
        let netuid = NetUid::from(1);
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            netuid, 1u16, // tempo
        );

        #[extrinsic_call]
        _(RawOrigin::Root, netuid, true);
    }

    #[benchmark]
    fn sudo_toggle_evm_precompile() {
        #[extrinsic_call]
        _(RawOrigin::Root, PrecompileEnum::Staking, true);
    }

    #[benchmark]
    fn sudo_set_subnet_moving_alpha() {
        #[extrinsic_call]
        _(RawOrigin::Root, 100u64.into());
    }

    #[benchmark]
    fn sudo_set_ema_price_halving_period() {
        let netuid = NetUid::from(1);

        pallet_subtensor::Pallet::<T>::init_new_network(
            netuid, 1u16, // tempo
        );

        #[extrinsic_call]
        _(RawOrigin::Root, netuid, 100u64);
    }

    #[benchmark]
    fn sudo_set_alpha_sigmoid_steepness() {
        let netuid = NetUid::from(1);
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            netuid, 1u16, // tempo
        );

        #[extrinsic_call]
        _(RawOrigin::Root, netuid, 100i16);
    }

    #[benchmark]
    fn sudo_set_yuma3_enabled() {
        let netuid = NetUid::from(1);
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            netuid, 1u16, // tempo
        );

        #[extrinsic_call]
        _(RawOrigin::Root, netuid, true);
    }

    #[benchmark]
    fn sudo_set_bonds_reset_enabled() {
        let netuid = NetUid::from(1);
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            netuid, 1u16, // tempo
        );

        #[extrinsic_call]
        _(RawOrigin::Root, netuid, true);
    }

    #[benchmark]
    fn sudo_set_subnet_emission_enabled() {
        let netuid = NetUid::from(1);
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            netuid, 1u16, // tempo
        );

        #[extrinsic_call]
        _(RawOrigin::Root, netuid, false);

        assert!(!pallet_subtensor::SubnetEmissionEnabled::<T>::get(netuid));
    }
    #[benchmark]
    fn sudo_set_sn_owner_hotkey() {
        let netuid = NetUid::from(1);
        let old_hotkey: T::AccountId = account("OldOwner", 0, 1);
        let hotkey: T::AccountId = account("Alice", 0, 1);

        pallet_subtensor::Pallet::<T>::init_new_network(
            netuid, 1u16, // tempo
        );
        pallet_subtensor::SubnetOwnerHotkey::<T>::insert(netuid, &old_hotkey);
        let now = pallet_subtensor::Pallet::<T>::get_current_block_as_u64();
        let old_owner_lock = LockState {
            locked_mass: 1_000u64.into(),
            conviction: U64F64::from_num(1_000),
            last_update: now,
        };
        let new_owner_lock = LockState {
            locked_mass: 2_000u64.into(),
            conviction: U64F64::from_num(2_000),
            last_update: now,
        };
        pallet_subtensor::Pallet::<T>::insert_owner_lock_state(netuid, old_owner_lock.clone());
        pallet_subtensor::Pallet::<T>::insert_decaying_owner_lock_state(netuid, old_owner_lock);
        pallet_subtensor::Pallet::<T>::insert_hotkey_lock_state(
            netuid,
            &hotkey,
            new_owner_lock.clone(),
        );
        pallet_subtensor::Pallet::<T>::insert_decaying_hotkey_lock_state(
            netuid,
            &hotkey,
            new_owner_lock,
        );

        #[extrinsic_call]
        _(RawOrigin::Root, netuid, hotkey.clone());

        assert_eq!(
            pallet_subtensor::SubnetOwnerHotkey::<T>::get(netuid),
            hotkey
        );
        assert!(pallet_subtensor::HotkeyLock::<T>::contains_key(
            netuid,
            &old_hotkey
        ));
        assert!(pallet_subtensor::DecayingHotkeyLock::<T>::contains_key(
            netuid,
            &old_hotkey
        ));
        assert!(pallet_subtensor::OwnerLock::<T>::contains_key(netuid));
        assert!(pallet_subtensor::DecayingOwnerLock::<T>::contains_key(
            netuid
        ));
    }

    #[benchmark]
    fn sudo_set_subtoken_enabled() {
        let netuid = NetUid::from(1);
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            netuid, 1u16, // tempo
        );

        #[extrinsic_call]
        _(RawOrigin::Root, netuid, true);
    }

    #[benchmark]
    fn sudo_set_admin_freeze_window() {
        #[extrinsic_call]
		_(RawOrigin::Root, 5u16/*window*/)/*sudo_set_admin_freeze_window*/;
    }

    #[benchmark]
    fn sudo_set_owner_hparam_rate_limit() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        #[extrinsic_call]
		_(RawOrigin::Root, 2u16/*epochs*/)/*sudo_set_owner_hparam_rate_limit*/;
    }

    #[benchmark]
    fn sudo_set_owner_immune_neuron_limit() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*sudo_tempo*/
        );

        #[extrinsic_call]
        _(RawOrigin::Root, 1u16.into()/*netuid*/, 5u16/*immune_neurons*/)/*sudo_set_owner_immune_neuron_limit()*/;
    }

    #[benchmark]
    fn sudo_trim_to_max_allowed_uids() {
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*sudo_tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 256u16/*max_n*/)/*sudo_trim_to_max_allowed_uids()*/;
    }

    #[benchmark]
    fn sudo_set_min_non_immune_uids() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        // create a network for netuid = 1
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /* netuid */
            1u16,        /* sudo_tempo */
        );

        #[extrinsic_call]
        _(
            RawOrigin::Root,
            1u16.into(), /* netuid */
            12u16,       /* min */
        ); /* sudo_set_min_non_immune_uids() */
    }

    #[benchmark]
    fn sudo_set_max_epochs_per_block() {
        #[extrinsic_call]
        _(RawOrigin::Root, 8u8);

        assert_eq!(
            pallet_subtensor::Pallet::<T>::get_max_epochs_per_block(),
            8u8
        );
    }

    fn setup_worst_case_admin_subnet<T: Config>(netuid: NetUid) -> T::AccountId {
        let owner: T::AccountId = whitelisted_caller();
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(netuid, 1u16);
        pallet_subtensor::Pallet::<T>::set_max_allowed_uids(netuid, 1);
        pallet_subtensor::SubnetOwner::<T>::insert(netuid, owner.clone());
        owner
    }

    fn max_emission_split() -> Vec<u16> {
        let mut split = vec![4096u16; 15];
        split.push(4095u16);
        split
    }

    #[benchmark]
    fn sudo_set_adjustment_alpha() {
        let netuid = NetUid::from(1);
        let owner = setup_worst_case_admin_subnet::<T>(netuid);

        #[extrinsic_call]
        _(RawOrigin::Signed(owner), netuid, u64::MAX);
    }

    #[benchmark]
    fn sudo_set_network_pow_registration_allowed() {
        #[block]
        {
            assert!(
                AdminUtils::<T>::sudo_set_network_pow_registration_allowed(
                    RawOrigin::Root.into(),
                    NetUid::from(u16::MAX),
                    true,
                )
                .is_err()
            );
        }
    }

    #[benchmark]
    fn sudo_set_subnet_owner_cut() {
        #[extrinsic_call]
        _(RawOrigin::Root, u16::MAX);
    }

    #[benchmark]
    fn sudo_set_network_rate_limit() {
        #[extrinsic_call]
        _(RawOrigin::Root, u64::MAX);
    }

    #[benchmark]
    fn sudo_set_network_immunity_period() {
        #[extrinsic_call]
        _(RawOrigin::Root, u64::MAX);
    }

    #[benchmark]
    fn sudo_set_network_min_lock_cost() {
        #[extrinsic_call]
        _(RawOrigin::Root, TaoBalance::from(u64::MAX));
    }

    #[benchmark]
    fn sudo_set_subnet_limit() {
        #[extrinsic_call]
        _(RawOrigin::Root, u16::MAX);
    }

    #[benchmark]
    fn sudo_set_lock_reduction_interval() {
        #[extrinsic_call]
        _(RawOrigin::Root, u64::MAX);
    }

    #[benchmark]
    fn sudo_set_evm_chain_id() {
        #[extrinsic_call]
        _(RawOrigin::Root, u64::MAX);
    }

    #[benchmark]
    fn sudo_set_recycle_or_burn() {
        let netuid = NetUid::from(1);
        let owner = setup_worst_case_admin_subnet::<T>(netuid);

        #[extrinsic_call]
        _(
            RawOrigin::Signed(owner),
            netuid,
            pallet_subtensor::RecycleOrBurnEnum::Burn,
        );
    }

    #[benchmark]
    fn sudo_set_ck_burn() {
        #[extrinsic_call]
        _(RawOrigin::Root, u64::MAX);
    }
    #[benchmark]
    fn sudo_set_mechanism_count() {
        let netuid = NetUid::from(1);
        let owner = setup_worst_case_admin_subnet::<T>(netuid);
        let mechanism_count = 16u8.into();
        assert!(pallet_subtensor::Pallet::<T>::do_set_max_mechanism_count(mechanism_count).is_ok());

        #[extrinsic_call]
        _(RawOrigin::Signed(owner), netuid, mechanism_count);
    }

    #[benchmark]
    fn sudo_set_mechanism_emission_split() {
        let netuid = NetUid::from(1);
        let owner = setup_worst_case_admin_subnet::<T>(netuid);
        let mechanism_count = 16u8.into();
        assert!(pallet_subtensor::Pallet::<T>::do_set_max_mechanism_count(mechanism_count).is_ok());
        assert!(
            pallet_subtensor::Pallet::<T>::do_set_mechanism_count(netuid, mechanism_count).is_ok()
        );

        #[extrinsic_call]
        _(RawOrigin::Signed(owner), netuid, Some(max_emission_split()));
    }

    #[benchmark]
    fn sudo_set_tao_flow_cutoff() {
        #[extrinsic_call]
        _(RawOrigin::Root, I64F64::from_num(i64::MAX));
    }

    #[benchmark]
    fn sudo_set_tao_flow_normalization_exponent() {
        #[extrinsic_call]
        _(RawOrigin::Root, U64F64::from_num(2));
    }

    #[benchmark]
    fn sudo_set_emission_bar_quantile() {
        #[extrinsic_call]
        _(RawOrigin::Root, U64F64::from_num(0.61));
    }

    #[benchmark]
    fn sudo_set_emission_bar_rank() {
        #[extrinsic_call]
        _(RawOrigin::Root, 64u16);
    }

    #[benchmark]
    fn sudo_set_emission_gate_exponent() {
        #[extrinsic_call]
        _(RawOrigin::Root, U64F64::from_num(3));
    }

    #[benchmark]
    fn sudo_set_tao_flow_smoothing_factor() {
        #[extrinsic_call]
        _(RawOrigin::Root, u64::MAX);
    }

    #[benchmark]
    fn sudo_set_net_tao_flow_enabled() {
        #[extrinsic_call]
        _(RawOrigin::Root, true);
    }

    #[benchmark]
    fn sudo_set_max_mechanism_count() {
        #[extrinsic_call]
        _(RawOrigin::Root, 16u8.into());
    }

    #[benchmark]
    fn sudo_set_start_call_delay() {
        #[extrinsic_call]
        _(RawOrigin::Root, u64::MAX);
    }

    #[benchmark]
    fn sudo_set_burn_half_life() {
        let netuid = NetUid::from(1);
        let owner = setup_worst_case_admin_subnet::<T>(netuid);
        let max_half_life = pallet_subtensor::MaxBurnHalfLife::<T>::get();

        #[extrinsic_call]
        _(RawOrigin::Signed(owner), netuid, max_half_life);
    }

    #[benchmark]
    fn sudo_set_burn_increase_mult() {
        let netuid = NetUid::from(1);
        let owner = setup_worst_case_admin_subnet::<T>(netuid);

        #[extrinsic_call]
        _(RawOrigin::Signed(owner), netuid, U64F64::from_num(3));
    }

    #[benchmark]
    fn sudo_set_collateral_lock_share() {
        let netuid = NetUid::from(1);
        let owner = setup_worst_case_admin_subnet::<T>(netuid);
        let max_share = pallet_subtensor::MaxCollateralLockShare::<T>::get();

        #[extrinsic_call]
        _(RawOrigin::Signed(owner), netuid, max_share);
    }

    #[benchmark]
    fn sudo_set_collateral_drain_ratio() {
        let netuid = NetUid::from(1);
        let owner = setup_worst_case_admin_subnet::<T>(netuid);

        #[extrinsic_call]
        _(RawOrigin::Signed(owner), netuid, U64F64::from_num(1));
    }

    #[benchmark]
    fn sudo_set_owner_cut_enabled() {
        let netuid = NetUid::from(1);
        let owner = setup_worst_case_admin_subnet::<T>(netuid);

        #[extrinsic_call]
        _(RawOrigin::Signed(owner), netuid, true);
    }

    #[benchmark]
    fn sudo_set_owner_cut_auto_lock_enabled() {
        let netuid = NetUid::from(1);
        let owner = setup_worst_case_admin_subnet::<T>(netuid);

        #[extrinsic_call]
        _(RawOrigin::Signed(owner), netuid, true);
    }

    impl_benchmark_test_suite!(AdminUtils, mock::new_test_ext(), mock::Test);
}
