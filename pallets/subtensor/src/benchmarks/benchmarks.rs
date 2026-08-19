//! Subtensor pallet benchmarking.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::expect_used
)]
#![cfg(feature = "runtime-benchmarks")]
#![allow(deprecated)]

use crate::Pallet as Subtensor;
use crate::staking::lock::LockState;
use crate::subnets::mechanism::GLOBAL_MAX_SUBNET_COUNT;
use crate::*;
use codec::{Compact, Encode};
use frame_benchmarking::v2::*;
use frame_support::{
    StorageDoubleMap, assert_ok,
    dispatch::{DispatchInfo, PostDispatchInfo},
    traits::{Get, IsSubType, OriginTrait},
};
use frame_system::{RawOrigin, pallet_prelude::BlockNumberFor};
pub use pallet::*;
use sp_core::{H160, H256, ecdsa};
use sp_runtime::{
    BoundedVec, PerU16, Percent,
    traits::{BlakeTwo256, Dispatchable, Hash},
};
use sp_std::collections::vec_deque::VecDeque;
use sp_std::vec;
use substrate_fixed::types::{I96F32, U64F64};
use subtensor_runtime_common::{AlphaBalance, NetUid, NetUidStorageIndex, TaoBalance};
use subtensor_swap_interface::SwapHandler;

mod helpers;

#[benchmarks(
    where
        T: pallet_balances::Config + pallet_shield::Config,
        <T as pallet_balances::Config>::ExistentialDeposit: Get<TaoBalance>,
        <T as frame_system::Config>::RuntimeCall:
            Dispatchable<RuntimeOrigin = OriginFor<T>, Info = DispatchInfo, PostInfo = PostDispatchInfo>
            + IsSubType<Call<T>>
            + IsSubType<pallet_shield::Call<T>>,
        OriginFor<T>: Clone + OriginTrait<AccountId = T::AccountId>,
)]
mod pallet_benchmarks {
    use super::helpers::*;
    use super::*;

    #[benchmark]
    fn register() {
        let netuid = NetUid::from(1);
        let hotkey: T::AccountId = account("register_hot", 0, 1);
        let coldkey: T::AccountId = account("register_cold", 0, 2);

        setup_full_subnet_registration_benchmark::<T>(
            netuid,
            "register_existing_hot",
            "register_existing_cold",
        );
        fund_for_registration::<T>(netuid, &coldkey);
        Subtensor::<T>::set_difficulty(netuid, 1);

        let block_number: u64 = Subtensor::<T>::get_current_block_as_u64();
        let (nonce, work): (u64, Vec<u8>) =
            Subtensor::<T>::create_work_for_block_number(netuid, block_number, 3, &hotkey);

        #[extrinsic_call]
        _(
            RawOrigin::Signed(coldkey.clone()),
            netuid,
            block_number,
            nonce,
            work,
            hotkey.clone(),
            coldkey.clone(),
        );
    }

    #[benchmark]
    fn set_weights() {
        let netuid = NetUid::from(1);
        let version_key: u64 = 1;
        let tempo: u16 = 1;

        Subtensor::<T>::init_new_network(netuid, tempo);
        Subtensor::<T>::set_max_allowed_uids(netuid, 4096);
        SubtokenEnabled::<T>::insert(netuid, true);
        Subtensor::<T>::set_network_registration_allowed(netuid, true);
        Subtensor::<T>::set_max_registrations_per_block(netuid, 4096);
        Subtensor::<T>::set_target_registrations_per_interval(netuid, 4096);
        Subtensor::<T>::set_commit_reveal_weights_enabled(netuid, false);
        SubnetTAO::<T>::insert(netuid, TaoBalance::from(1_000_000_000_000_u64));
        SubnetAlphaIn::<T>::insert(netuid, AlphaBalance::from(1_000_000_000_000_000_u64));
        Subtensor::<T>::set_weights_set_rate_limit(netuid, 0);

        let mut seed: u32 = 1;
        let mut dests = Vec::new();
        let mut weights = Vec::new();
        let signer: T::AccountId = account("Alice", 0, seed);

        for _ in 0..4096 {
            let hotkey: T::AccountId = account("Alice", 0, seed);
            let coldkey: T::AccountId = account("Test", 0, seed);
            seed += 1;

            Subtensor::<T>::set_burn(netuid, 1.into());

            // Ensure enough for registration + minimum stake.
            fund_for_registration::<T>(netuid, &coldkey);

            RegistrationsThisInterval::<T>::insert(netuid, 0);

            // Reset burn so that we don't hit maximum issuance
            Burn::<T>::insert(netuid, TaoBalance::from(1_000_000));

            assert_ok!(Subtensor::<T>::burned_register(
                RawOrigin::Signed(coldkey.clone()).into(),
                netuid,
                hotkey.clone()
            ));

            let uid = Subtensor::<T>::get_uid_for_net_and_hotkey(netuid, &hotkey).unwrap();
            Subtensor::<T>::set_validator_permit_for_uid(netuid, uid, true);

            dests.push(uid);
            weights.push(uid);
        }

        #[extrinsic_call]
        _(
            RawOrigin::Signed(signer.clone()),
            netuid,
            dests,
            weights,
            version_key,
        );
    }

    #[benchmark]
    fn add_stake() {
        let netuid = NetUid::from(1);
        let tempo: u16 = 1;

        Subtensor::<T>::init_new_network(netuid, tempo);
        SubtokenEnabled::<T>::insert(netuid, true);
        Subtensor::<T>::set_burn(netuid, benchmark_registration_burn());
        Subtensor::<T>::set_network_registration_allowed(netuid, true);
        Subtensor::<T>::set_max_allowed_uids(netuid, 4096);

        let seed: u32 = 1;
        let coldkey: T::AccountId = account("Test", 0, seed);
        let hotkey: T::AccountId = account("Alice", 0, seed);
        let total_stake = TaoBalance::from(1_000_000_000);
        let amount = TaoBalance::from(60_000_000);

        seed_swap_reserves::<T>(netuid);
        add_balance_to_coldkey_account::<T>(&coldkey, total_stake.into());
        add_lock::<T>(&coldkey, netuid);

        assert_ok!(Subtensor::<T>::burned_register(
            RawOrigin::Signed(coldkey.clone()).into(),
            netuid,
            hotkey.clone()
        ));

        #[extrinsic_call]
        _(
            RawOrigin::Signed(coldkey.clone()),
            hotkey.clone(),
            netuid,
            amount,
        );
    }

    #[benchmark]
    fn serve_axon() {
        let netuid = NetUid::from(1);
        let caller: T::AccountId = whitelisted_caller();
        let version: u32 = 2;
        let ip: u128 = 1676056785;
        let port: u16 = 128;
        let ip_type: u8 = 4;
        let protocol: u8 = 0;
        let placeholder1: u8 = 0;
        let placeholder2: u8 = 0;

        Subtensor::<T>::init_new_network(netuid, 1);
        SubtokenEnabled::<T>::insert(netuid, true);
        Subtensor::<T>::set_network_registration_allowed(netuid, true);
        Subtensor::<T>::set_max_allowed_uids(netuid, 4096);

        Subtensor::<T>::set_burn(netuid, benchmark_registration_burn());
        seed_swap_reserves::<T>(netuid);
        fund_for_registration::<T>(netuid, &caller);

        assert_ok!(Subtensor::<T>::burned_register(
            RawOrigin::Signed(caller.clone()).into(),
            netuid,
            caller.clone()
        ));

        Subtensor::<T>::set_serving_rate_limit(netuid, 0);

        #[extrinsic_call]
        _(
            RawOrigin::Signed(caller.clone()),
            netuid,
            version,
            ip,
            port,
            ip_type,
            protocol,
            placeholder1,
            placeholder2,
        );
    }

    #[benchmark]
    fn serve_prometheus() {
        let netuid = NetUid::from(1);
        let caller: T::AccountId = whitelisted_caller();
        let version: u32 = 2;
        let ip: u128 = 1676056785;
        let port: u16 = 128;
        let ip_type: u8 = 4;

        Subtensor::<T>::init_new_network(netuid, 1);
        SubtokenEnabled::<T>::insert(netuid, true);
        Subtensor::<T>::set_network_registration_allowed(netuid, true);
        Subtensor::<T>::set_max_allowed_uids(netuid, 4096);

        Subtensor::<T>::set_burn(netuid, benchmark_registration_burn());
        seed_swap_reserves::<T>(netuid);
        fund_for_registration::<T>(netuid, &caller);

        assert_ok!(Subtensor::<T>::burned_register(
            RawOrigin::Signed(caller.clone()).into(),
            netuid,
            caller.clone()
        ));

        Subtensor::<T>::set_serving_rate_limit(netuid, 0);

        #[extrinsic_call]
        _(
            RawOrigin::Signed(caller.clone()),
            netuid,
            version,
            ip,
            port,
            ip_type,
        );
    }

    #[benchmark]
    fn burned_register() {
        let netuid = NetUid::from(1);
        let hotkey: T::AccountId = account("burned_register_hot", 0, 1);
        let coldkey: T::AccountId = account("burned_register_cold", 0, 1);

        setup_full_subnet_registration_benchmark::<T>(
            netuid,
            "burned_register_existing_hot",
            "burned_register_existing_cold",
        );
        // Worst case: collateral enabled, so the charge also stakes-and-locks the
        // collateral share (AMM swap + share-pool + MinerCollateral write) rather
        // than only burning.
        CollateralLockShare::<T>::insert(netuid, MaxCollateralLockShare::<T>::get());
        fund_for_registration::<T>(netuid, &coldkey);

        #[extrinsic_call]
        _(RawOrigin::Signed(coldkey.clone()), netuid, hotkey.clone());
    }

    #[benchmark]
    fn root_register() {
        let coldkey: T::AccountId = account("root_register_cold", 0, 1);
        let hotkey: T::AccountId = account("root_register_hot", 0, 1);

        setup_full_root_registration_benchmark::<T>();
        // Root admission is burn-based: fund the coldkey for the burn charge
        // so the recycle path (the worst case) is exercised.
        fund_for_registration::<T>(NetUid::ROOT, &coldkey);

        #[extrinsic_call]
        _(RawOrigin::Signed(coldkey.clone()), hotkey.clone());
    }

    #[benchmark]
    fn register_network() {
        let seed: u32 = 1;
        let coldkey: T::AccountId = account("Test", 0, seed);
        let hotkey: T::AccountId = account("TestHotkey", 0, seed);

        Subtensor::<T>::set_network_rate_limit(1);
        let amount: u64 = 100_000_000_000_000u64.saturating_mul(2);
        add_balance_to_coldkey_account::<T>(&coldkey, amount.into());

        #[extrinsic_call]
        _(RawOrigin::Signed(coldkey.clone()), hotkey.clone());
    }

    #[benchmark]
    fn commit_weights() {
        let tempo: u16 = 1;
        let netuid = NetUid::from(1);
        let version_key: u64 = 0;
        let uids: Vec<u16> = vec![0];
        let weight_values: Vec<u16> = vec![10];
        let hotkey: T::AccountId = account("hot", 0, 1);
        let coldkey: T::AccountId = account("cold", 0, 2);

        let commit_hash: H256 = BlakeTwo256::hash_of(&(
            hotkey.clone(),
            netuid,
            uids.clone(),
            weight_values.clone(),
            version_key,
        ));

        Subtensor::<T>::init_new_network(netuid, tempo);
        Subtensor::<T>::set_network_registration_allowed(netuid, true);
        Subtensor::<T>::set_weights_set_rate_limit(netuid, 0);
        Subtensor::<T>::set_difficulty(netuid, 1);
        SubtokenEnabled::<T>::insert(netuid, true);

        Subtensor::<T>::set_burn(netuid, benchmark_registration_burn());
        seed_swap_reserves::<T>(netuid);
        fund_for_registration::<T>(netuid, &coldkey);

        assert_ok!(Subtensor::<T>::burned_register(
            RawOrigin::Signed(coldkey.clone()).into(),
            netuid,
            hotkey.clone()
        ));

        Subtensor::<T>::set_validator_permit_for_uid(netuid, 0, true);
        Subtensor::<T>::set_commit_reveal_weights_enabled(netuid, true);

        #[extrinsic_call]
        _(RawOrigin::Signed(hotkey.clone()), netuid, commit_hash);
    }

    #[benchmark]
    fn reveal_weights() {
        let tempo: u16 = 0;
        let netuid = NetUid::from(1);
        let version_key: u64 = 0;
        let uids: Vec<u16> = vec![0];
        let weight_values: Vec<u16> = vec![10];
        let salt: Vec<u16> = vec![8];
        let hotkey: T::AccountId = account("hot", 0, 1);
        let coldkey: T::AccountId = account("cold", 1, 2);

        Subtensor::<T>::init_new_network(netuid, tempo);
        Subtensor::<T>::set_network_registration_allowed(netuid, true);
        SubtokenEnabled::<T>::insert(netuid, true);
        Subtensor::<T>::set_weights_set_rate_limit(netuid, 0);
        Subtensor::<T>::set_difficulty(netuid, 1);

        Subtensor::<T>::set_burn(netuid, benchmark_registration_burn());
        seed_swap_reserves::<T>(netuid);
        fund_for_registration::<T>(netuid, &coldkey);

        assert_ok!(Subtensor::<T>::burned_register(
            RawOrigin::Signed(coldkey.clone()).into(),
            netuid,
            hotkey.clone()
        ));

        Subtensor::<T>::set_validator_permit_for_uid(netuid, 0, true);
        Subtensor::<T>::set_commit_reveal_weights_enabled(netuid, true);

        let commit_hash: H256 = BlakeTwo256::hash_of(&(
            hotkey.clone(),
            netuid,
            uids.clone(),
            weight_values.clone(),
            salt.clone(),
            version_key,
        ));
        assert_ok!(Subtensor::<T>::commit_weights(
            RawOrigin::Signed(hotkey.clone()).into(),
            netuid,
            commit_hash,
        ));

        // Advance the epoch counter into the commit's reveal window.
        let reveal_period = Subtensor::<T>::get_reveal_period(netuid);
        SubnetEpochIndex::<T>::mutate(netuid, |e| *e = e.saturating_add(reveal_period));

        #[extrinsic_call]
        _(
            RawOrigin::Signed(hotkey.clone()),
            netuid,
            uids.clone(),
            weight_values.clone(),
            salt.clone(),
            version_key,
        );
    }

    #[benchmark]
    fn sudo_set_tx_childkey_take_rate_limit() {
        let new_rate_limit: u64 = 100;

        #[extrinsic_call]
        _(RawOrigin::Root, new_rate_limit);
    }

    #[benchmark]
    fn set_childkey_take() {
        let netuid = NetUid::from(1);
        let coldkey: T::AccountId = account("Cold", 0, 1);
        let hotkey: T::AccountId = account("Hot", 0, 1);
        let take = PerU16::from_parts(1000);

        Subtensor::<T>::init_new_network(netuid, 1);
        Subtensor::<T>::set_network_registration_allowed(netuid, true);
        SubtokenEnabled::<T>::insert(netuid, true);

        Subtensor::<T>::set_burn(netuid, benchmark_registration_burn());
        seed_swap_reserves::<T>(netuid);
        fund_for_registration::<T>(netuid, &coldkey);

        assert_ok!(Subtensor::<T>::burned_register(
            RawOrigin::Signed(coldkey.clone()).into(),
            netuid,
            hotkey.clone()
        ));

        #[extrinsic_call]
        _(
            RawOrigin::Signed(coldkey.clone()),
            hotkey.clone(),
            netuid,
            take,
        );
    }

    #[benchmark]
    fn announce_coldkey_swap() {
        let coldkey: T::AccountId = account("old_coldkey", 0, 0);
        let new_coldkey: T::AccountId = account("new_coldkey", 0, 0);
        let new_coldkey_hash: T::Hash = <T as frame_system::Config>::Hashing::hash_of(&new_coldkey);

        let ed = <T as pallet_balances::Config>::ExistentialDeposit::get();
        let swap_cost = Subtensor::<T>::get_key_swap_cost();
        add_balance_to_coldkey_account::<T>(&coldkey, swap_cost + ed);

        #[extrinsic_call]
        _(RawOrigin::Signed(coldkey), new_coldkey_hash);
    }

    #[benchmark]
    fn swap_coldkey_announced() {
        let old_coldkey: T::AccountId = account("old_coldkey", 0, 0);
        let new_coldkey: T::AccountId = account("new_coldkey", 0, 0);
        let new_coldkey_hash: T::Hash = <T as frame_system::Config>::Hashing::hash_of(&new_coldkey);
        let hotkey1: T::AccountId = account("hotkey1", 0, 0);

        let now = frame_system::Pallet::<T>::block_number();
        let delay = ColdkeySwapAnnouncementDelay::<T>::get();
        ColdkeySwapAnnouncements::<T>::insert(&old_coldkey, (now, new_coldkey_hash));
        frame_system::Pallet::<T>::set_block_number(now + delay + 1u32.into());

        let netuid = NetUid::from(1);
        Subtensor::<T>::init_new_network(netuid, 1);
        Subtensor::<T>::set_network_registration_allowed(netuid, true);
        SubtokenEnabled::<T>::insert(netuid, true);
        Subtensor::<T>::set_burn(netuid, benchmark_registration_burn());
        seed_swap_reserves::<T>(netuid);
        fund_for_registration::<T>(netuid, &old_coldkey);
        Subtensor::<T>::set_difficulty(netuid, 1);

        let block_number = Subtensor::<T>::get_current_block_as_u64();
        let (nonce, work) =
            Subtensor::<T>::create_work_for_block_number(netuid, block_number, 3, &hotkey1);

        assert_ok!(Subtensor::<T>::register(
            RawOrigin::Signed(old_coldkey.clone()).into(),
            netuid,
            block_number,
            nonce,
            work.clone(),
            hotkey1.clone(),
            old_coldkey.clone(),
        ));

        // Worst case: migrate the full bounded collateral-hotkey index plus
        // lineage / aggregate updates that the legacy weight omitted.
        let locked = AlphaBalance::from(1_000_000_u64);
        seed_miner_collateral_position::<T>(netuid, &hotkey1, &old_coldkey, locked);
        for i in 1..MAX_COLDKEY_COLLATERAL_HOTKEYS {
            let extra_hot: T::AccountId = account("collateral_hot", i, 0);
            Owner::<T>::insert(&extra_hot, &old_coldkey);
            seed_miner_collateral_position::<T>(netuid, &extra_hot, &old_coldkey, locked);
        }

        #[extrinsic_call]
        _(RawOrigin::Signed(old_coldkey), new_coldkey);
    }

    #[benchmark]
    fn swap_coldkey() {
        let old_coldkey: T::AccountId = account("old_coldkey", 0, 0);
        let new_coldkey: T::AccountId = account("new_coldkey", 0, 0);
        let hotkey1: T::AccountId = account("hotkey1", 0, 0);
        let netuid = NetUid::from(1);

        let swap_cost = Subtensor::<T>::get_key_swap_cost();
        let free_balance_old = swap_cost + TaoBalance::from(12_345_u64);

        Subtensor::<T>::init_new_network(netuid, 1);
        Subtensor::<T>::set_network_registration_allowed(netuid, true);
        Subtensor::<T>::set_difficulty(netuid, 1);
        SubtokenEnabled::<T>::insert(netuid, true);
        Subtensor::<T>::set_burn(netuid, benchmark_registration_burn());
        seed_swap_reserves::<T>(netuid);

        fund_for_registration::<T>(netuid, &old_coldkey);

        assert_ok!(Subtensor::<T>::burned_register(
            RawOrigin::Signed(old_coldkey.clone()).into(),
            netuid,
            hotkey1.clone(),
        ));

        add_balance_to_coldkey_account::<T>(&old_coldkey, free_balance_old);
        let name: Vec<u8> = b"The fourth Coolest Identity".to_vec();
        let identity = ChainIdentityV2 {
            name,
            url: vec![],
            github_repo: vec![],
            image: vec![],
            discord: vec![],
            description: vec![],
            additional: vec![],
        };
        IdentitiesV2::<T>::insert(&old_coldkey, identity);

        // Worst case: migrate the full bounded collateral-hotkey index plus
        // lineage / aggregate updates that the legacy weight omitted.
        let locked = AlphaBalance::from(1_000_000_u64);
        seed_miner_collateral_position::<T>(netuid, &hotkey1, &old_coldkey, locked);
        for i in 1..MAX_COLDKEY_COLLATERAL_HOTKEYS {
            let extra_hot: T::AccountId = account("collateral_hot", i, 0);
            Owner::<T>::insert(&extra_hot, &old_coldkey);
            seed_miner_collateral_position::<T>(netuid, &extra_hot, &old_coldkey, locked);
        }

        #[extrinsic_call]
        _(
            RawOrigin::Root,
            old_coldkey.clone(),
            new_coldkey.clone(),
            swap_cost,
        );
    }

    #[benchmark]
    fn dispute_coldkey_swap() {
        let coldkey: T::AccountId = account("old_coldkey", 0, 0);
        let coldkey_hash: T::Hash = <T as frame_system::Config>::Hashing::hash_of(&coldkey);
        let now = frame_system::Pallet::<T>::block_number();

        ColdkeySwapAnnouncements::<T>::insert(&coldkey, (now, coldkey_hash));

        #[extrinsic_call]
        _(RawOrigin::Signed(coldkey));
    }

    #[benchmark]
    fn clear_coldkey_swap_announcement() {
        let coldkey: T::AccountId = account("coldkey", 0, 0);
        let new_coldkey: T::AccountId = account("new_coldkey", 0, 0);
        let new_coldkey_hash: T::Hash = <T as frame_system::Config>::Hashing::hash_of(&new_coldkey);
        let now = frame_system::Pallet::<T>::block_number();
        let delay = ColdkeySwapReannouncementDelay::<T>::get();

        ColdkeySwapAnnouncements::<T>::insert(&coldkey, (now, new_coldkey_hash));
        frame_system::Pallet::<T>::set_block_number(now + delay);

        #[extrinsic_call]
        _(RawOrigin::Signed(coldkey));
    }

    #[benchmark]
    fn reset_coldkey_swap() {
        let coldkey: T::AccountId = account("old_coldkey", 0, 0);
        let coldkey_hash: T::Hash = <T as frame_system::Config>::Hashing::hash_of(&coldkey);
        let now = frame_system::Pallet::<T>::block_number();

        ColdkeySwapAnnouncements::<T>::insert(&coldkey, (now, coldkey_hash));
        ColdkeySwapDisputes::<T>::insert(&coldkey, now);

        #[extrinsic_call]
        _(RawOrigin::Root, coldkey);
    }

    #[benchmark]
    fn batch_reveal_weights() {
        let tempo: u16 = 0;
        let netuid = NetUid::from(1);
        let num_commits: usize = 10;

        let hotkey: T::AccountId = account("hot", 0, 1);
        let coldkey: T::AccountId = account("cold", 0, 2);

        Subtensor::<T>::init_new_network(netuid, tempo);
        Subtensor::<T>::set_network_registration_allowed(netuid, true);
        Subtensor::<T>::set_commit_reveal_weights_enabled(netuid, true);
        Subtensor::<T>::set_weights_set_rate_limit(netuid, 0);
        Subtensor::<T>::set_difficulty(netuid, 1);
        SubtokenEnabled::<T>::insert(netuid, true);

        Subtensor::<T>::set_burn(netuid, benchmark_registration_burn());
        seed_swap_reserves::<T>(netuid);
        fund_for_registration::<T>(netuid, &coldkey);

        assert_ok!(Subtensor::<T>::burned_register(
            RawOrigin::Signed(coldkey.clone()).into(),
            netuid,
            hotkey.clone()
        ));

        Subtensor::<T>::set_validator_permit_for_uid(netuid, 0, true);

        let mut uids_list = Vec::new();
        let mut values_list = Vec::new();
        let mut salts_list = Vec::new();
        let mut version_keys = Vec::new();

        for i in 0..num_commits {
            let uids = vec![0u16];
            let values = vec![i as u16];
            let salts = vec![i as u16];
            let version_key_i: u64 = i as u64;

            let commit_hash: H256 = BlakeTwo256::hash_of(&(
                hotkey.clone(),
                netuid,
                uids.clone(),
                values.clone(),
                salts.clone(),
                version_key_i,
            ));

            assert_ok!(Subtensor::<T>::commit_weights(
                RawOrigin::Signed(hotkey.clone()).into(),
                netuid,
                commit_hash
            ));

            uids_list.push(uids);
            values_list.push(values);
            salts_list.push(salts);
            version_keys.push(version_key_i);
        }

        // Advance the epoch counter into the reveal window for these commits.
        let reveal_period = Subtensor::<T>::get_reveal_period(netuid);
        SubnetEpochIndex::<T>::mutate(netuid, |e| *e = e.saturating_add(reveal_period));

        #[extrinsic_call]
        _(
            RawOrigin::Signed(hotkey.clone()),
            netuid,
            uids_list,
            values_list,
            salts_list,
            version_keys,
        );
    }

    #[benchmark]
    fn recycle_alpha() {
        let netuid = NetUid::from(1);

        let coldkey: T::AccountId = account("Test", 0, 1);
        let hotkey: T::AccountId = account("Alice", 0, 1);

        Subtensor::<T>::init_new_network(netuid, 1);
        SubtokenEnabled::<T>::insert(netuid, true);
        Subtensor::<T>::set_network_registration_allowed(netuid, true);
        Subtensor::<T>::set_burn(netuid, benchmark_registration_burn());

        fund_for_registration::<T>(netuid, &coldkey);

        assert_ok!(Subtensor::<T>::burned_register(
            RawOrigin::Signed(coldkey.clone()).into(),
            netuid,
            hotkey.clone()
        ));

        let alpha_amount = AlphaBalance::from(1_000_000);
        SubnetAlphaOut::<T>::insert(netuid, alpha_amount * 2.into());

        Subtensor::<T>::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid,
            alpha_amount,
        );

        assert_eq!(
            TotalHotkeyAlpha::<T>::get(&hotkey, netuid),
            alpha_amount.into()
        );

        #[extrinsic_call]
        _(
            RawOrigin::Signed(coldkey.clone()),
            hotkey.clone(),
            alpha_amount,
            netuid,
        );
    }

    #[benchmark]
    fn burn_alpha() {
        let netuid = NetUid::from(1);
        let coldkey: T::AccountId = account("Test", 0, 1);
        let hotkey: T::AccountId = account("Alice", 0, 1);

        Subtensor::<T>::init_new_network(netuid, 1);
        SubtokenEnabled::<T>::insert(netuid, true);
        Subtensor::<T>::set_network_registration_allowed(netuid, true);
        Subtensor::<T>::set_burn(netuid, benchmark_registration_burn());

        fund_for_registration::<T>(netuid, &coldkey);

        assert_ok!(Subtensor::<T>::burned_register(
            RawOrigin::Signed(coldkey.clone()).into(),
            netuid,
            hotkey.clone()
        ));

        let alpha_amount = 1_000_000;
        SubnetAlphaOut::<T>::insert(netuid, AlphaBalance::from(alpha_amount * 2));
        Subtensor::<T>::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid,
            alpha_amount.into(),
        );

        assert_eq!(
            TotalHotkeyAlpha::<T>::get(&hotkey, netuid),
            alpha_amount.into()
        );

        #[extrinsic_call]
        _(
            RawOrigin::Signed(coldkey.clone()),
            hotkey.clone(),
            alpha_amount.into(),
            netuid,
        );
    }

    #[benchmark]
    fn block_step() {
        setup_block_step_benchmark::<T>();

        #[block]
        {
            assert_ok!(Subtensor::<T>::block_step());
        }
    }
    #[benchmark]
    fn start_call() {
        let netuid = NetUid::from(1);
        let coldkey: T::AccountId = account("Test", 0, 1);
        let hotkey: T::AccountId = account("Alice", 0, 1);

        Subtensor::<T>::init_new_network(netuid, 1);
        SubtokenEnabled::<T>::insert(netuid, true);
        Subtensor::<T>::set_network_registration_allowed(netuid, true);

        Subtensor::<T>::set_burn(netuid, benchmark_registration_burn());
        seed_swap_reserves::<T>(netuid);
        fund_for_registration::<T>(netuid, &coldkey);
        SubnetOwner::<T>::set(netuid, coldkey.clone());

        assert_ok!(Subtensor::<T>::burned_register(
            RawOrigin::Signed(coldkey.clone()).into(),
            netuid,
            hotkey.clone()
        ));

        assert_eq!(SubnetOwner::<T>::get(netuid), coldkey.clone());
        assert_eq!(FirstEmissionBlockNumber::<T>::get(netuid), None);

        let current_block: u64 = Subtensor::<T>::get_current_block_as_u64();
        let duration = StartCallDelay::<T>::get();
        let block: BlockNumberFor<T> = (current_block + duration)
            .try_into()
            .ok()
            .expect("can't convert to block number");
        frame_system::Pallet::<T>::set_block_number(block);

        #[extrinsic_call]
        _(RawOrigin::Signed(coldkey.clone()), netuid);
    }

    #[benchmark]
    fn add_stake_limit() {
        let netuid = NetUid::from(1);
        let tempo: u16 = 1;
        let seed: u32 = 1;

        Subtensor::<T>::init_new_network(netuid, tempo);
        SubtokenEnabled::<T>::insert(netuid, true);
        Subtensor::<T>::set_burn(netuid, benchmark_registration_burn());
        Subtensor::<T>::set_network_registration_allowed(netuid, true);
        Subtensor::<T>::set_max_allowed_uids(netuid, 4096);

        let coldkey: T::AccountId = account("Test", 0, seed);
        let hotkey: T::AccountId = account("Alice", 0, seed);

        let initial_balance = TaoBalance::from(900_000_000_000_u64);
        add_balance_to_coldkey_account::<T>(&coldkey.clone(), initial_balance);
        add_lock::<T>(&coldkey, netuid);

        // Price = 0.01
        let tao_reserve = TaoBalance::from(1_000_000_000_000_u64);
        let alpha_in = AlphaBalance::from(100_000_000_000_000_u64);
        set_reserves::<T>(netuid, tao_reserve, alpha_in);

        assert_ok!(Subtensor::<T>::burned_register(
            RawOrigin::Signed(coldkey.clone()).into(),
            netuid,
            hotkey.clone()
        ));

        // Read current price and set limit price 0.1% higher, which is certainly getting hit
        // by swapping 100 TAO
        let current_price = T::SwapInterface::current_alpha_price(netuid);
        let limit = current_price
            .saturating_mul(U64F64::saturating_from_num(1_001_000_000))
            .saturating_to_num::<u64>()
            .into();
        let amount_to_be_staked = TaoBalance::from(100_000_000_000_u64);

        // Allow partial (worst case)
        #[extrinsic_call]
        _(
            RawOrigin::Signed(coldkey.clone()),
            hotkey,
            netuid,
            amount_to_be_staked,
            limit,
            true,
        );
    }

    #[benchmark]
    fn move_stake() {
        let coldkey: T::AccountId = whitelisted_caller();
        let origin: T::AccountId = account("A", 0, 1);
        let destination: T::AccountId = account("B", 0, 2);
        let netuid = NetUid::from(1);

        SubtokenEnabled::<T>::insert(netuid, true);
        Subtensor::<T>::init_new_network(netuid, 1);
        Subtensor::<T>::set_network_registration_allowed(netuid, true);

        let burn_fee = Subtensor::<T>::get_burn(netuid);
        let stake_tao = DefaultMinStake::<T>::get().saturating_mul(10.into());
        let deposit = burn_fee.saturating_mul(2.into()).saturating_add(stake_tao);
        add_balance_to_coldkey_account::<T>(&coldkey, deposit.into());
        add_lock::<T>(&coldkey, netuid);

        assert_ok!(Subtensor::<T>::burned_register(
            RawOrigin::Signed(coldkey.clone()).into(),
            netuid,
            origin.clone()
        ));

        set_reserves::<T>(netuid, deposit, AlphaBalance::from(deposit.to_u64()));
        TotalStake::<T>::set(deposit);

        assert_ok!(Subtensor::<T>::add_stake_limit(
            RawOrigin::Signed(coldkey.clone()).into(),
            origin.clone(),
            netuid,
            stake_tao,
            TaoBalance::MAX,
            false
        ));

        let alpha_to_move =
            Subtensor::<T>::get_stake_for_hotkey_and_coldkey_on_subnet(&origin, &coldkey, netuid);

        let _ = Subtensor::<T>::create_account_if_non_existent(&coldkey, &destination);

        #[extrinsic_call]
        _(
            RawOrigin::Signed(coldkey.clone()),
            origin.clone(),
            destination.clone(),
            netuid,
            netuid,
            alpha_to_move,
        );

        let staking_hotkeys = StakingHotkeys::<T>::get(&coldkey);
        assert!(!staking_hotkeys.contains(&origin));
        assert!(staking_hotkeys.contains(&destination));
    }

    #[benchmark]
    fn remove_stake() {
        let netuid = NetUid::from(1);
        let tempo: u16 = 1;
        let seed: u32 = 1;

        Subtensor::<T>::increase_total_stake(1_000_000_000_000_u64.into());

        Subtensor::<T>::init_new_network(netuid, tempo);
        Subtensor::<T>::set_network_registration_allowed(netuid, true);
        SubtokenEnabled::<T>::insert(netuid, true);

        Subtensor::<T>::set_max_allowed_uids(netuid, 4096);
        assert_eq!(Subtensor::<T>::get_max_allowed_uids(netuid), 4096);

        let coldkey: T::AccountId = account("Test", 0, seed);
        let hotkey: T::AccountId = account("Alice", 0, seed);
        Subtensor::<T>::set_burn(netuid, benchmark_registration_burn());

        // Price = 0.01
        let tao_reserve = TaoBalance::from(1_000_000_000_000_u64);
        let alpha_in = AlphaBalance::from(100_000_000_000_000_u64);
        set_reserves::<T>(netuid, tao_reserve, alpha_in);

        // Registration now requires keep-alive coverage of the burn; fund
        // above burn + ED rather than exactly the burn amount.
        fund_for_registration::<T>(netuid, &coldkey);

        assert_ok!(Subtensor::<T>::burned_register(
            RawOrigin::Signed(coldkey.clone()).into(),
            netuid,
            hotkey.clone()
        ));

        let staked_amt = TaoBalance::from(100_000_000_000_u64);
        add_balance_to_coldkey_account::<T>(&coldkey.clone(), staked_amt);

        assert_ok!(Subtensor::<T>::add_stake(
            RawOrigin::Signed(coldkey.clone()).into(),
            hotkey.clone(),
            netuid,
            staked_amt
        ));

        // Remove the complete relationship so the benchmark includes the
        // StakingHotkeys cleanup path.
        let amount_unstaked =
            Subtensor::<T>::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);

        #[extrinsic_call]
        _(
            RawOrigin::Signed(coldkey.clone()),
            hotkey.clone(),
            netuid,
            amount_unstaked,
        );

        assert!(!StakingHotkeys::<T>::contains_key(&coldkey));
    }

    #[benchmark]
    fn remove_stake_limit() {
        let netuid = NetUid::from(1);
        let tempo: u16 = 1;
        let seed: u32 = 1;

        Subtensor::<T>::increase_total_stake(1_000_000_000_000_u64.into());

        Subtensor::<T>::init_new_network(netuid, tempo);
        Subtensor::<T>::set_network_registration_allowed(netuid, true);
        SubtokenEnabled::<T>::insert(netuid, true);

        Subtensor::<T>::set_max_allowed_uids(netuid, 4096);
        assert_eq!(Subtensor::<T>::get_max_allowed_uids(netuid), 4096);

        let coldkey: T::AccountId = account("Test", 0, seed);
        let hotkey: T::AccountId = account("Alice", 0, seed);
        Subtensor::<T>::set_burn(netuid, benchmark_registration_burn());

        let tao_reserve = TaoBalance::from(1_000_000_000_000_u64);
        let alpha_in = AlphaBalance::from(100_000_000_000_000_u64);
        set_reserves::<T>(netuid, tao_reserve, alpha_in);

        // Registration now requires keep-alive coverage of the burn.
        fund_for_registration::<T>(netuid, &coldkey);
        add_lock::<T>(&coldkey, netuid);

        assert_ok!(Subtensor::<T>::burned_register(
            RawOrigin::Signed(coldkey.clone()).into(),
            netuid,
            hotkey.clone()
        ));

        let staked_amt = TaoBalance::from(100_000_000_000_u64);
        add_balance_to_coldkey_account::<T>(&coldkey.clone(), staked_amt);

        assert_ok!(Subtensor::<T>::add_stake(
            RawOrigin::Signed(coldkey.clone()).into(),
            hotkey.clone(),
            netuid,
            staked_amt
        ));

        // Remove the complete relationship so the benchmark includes the
        // StakingHotkeys cleanup path. A permissive limit keeps this on the
        // successful full-fill path.
        let amount_unstaked =
            Subtensor::<T>::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);
        let limit = T::SwapInterface::min_price();

        #[extrinsic_call]
        _(
            RawOrigin::Signed(coldkey.clone()),
            hotkey.clone(),
            netuid,
            amount_unstaked,
            limit,
            true,
        );

        assert!(!StakingHotkeys::<T>::contains_key(&coldkey));
    }

    #[benchmark]
    fn swap_stake_limit() {
        let coldkey: T::AccountId = whitelisted_caller::<AccountIdOf<T>>();
        let hot: T::AccountId = account("A", 0, 1);
        let netuid1 = NetUid::from(1);
        let netuid2 = NetUid::from(2);
        let allow: bool = true;

        SubtokenEnabled::<T>::insert(netuid1, true);
        Subtensor::<T>::init_new_network(netuid1, 1);
        Subtensor::<T>::set_network_registration_allowed(netuid1, true);

        SubtokenEnabled::<T>::insert(netuid2, true);
        Subtensor::<T>::init_new_network(netuid2, 1);
        Subtensor::<T>::set_network_registration_allowed(netuid2, true);

        let tao_reserve = TaoBalance::from(150_000_000_000_u64);
        let alpha_in = AlphaBalance::from(100_000_000_000_u64);
        set_reserves::<T>(netuid1, tao_reserve, alpha_in);
        SubnetTAO::<T>::insert(netuid2, tao_reserve);

        Subtensor::<T>::increase_total_stake(1_000_000_000_000_u64.into());

        let amount = TaoBalance::from(900_000_000_000_u64);
        let limit_stake = TaoBalance::from(6_000_000_000_u64);
        let amount_to_be_staked = TaoBalance::from(440_000_000_000_u64);
        add_balance_to_coldkey_account::<T>(&coldkey.clone(), amount);
        add_lock::<T>(&coldkey, netuid1);
        add_lock::<T>(&coldkey, netuid2);

        assert_ok!(Subtensor::<T>::burned_register(
            RawOrigin::Signed(coldkey.clone()).into(),
            netuid1,
            hot.clone()
        ));
        assert_ok!(Subtensor::<T>::burned_register(
            RawOrigin::Signed(coldkey.clone()).into(),
            netuid2,
            hot.clone()
        ));

        assert_ok!(Subtensor::<T>::add_stake_limit(
            RawOrigin::Signed(coldkey.clone()).into(),
            hot.clone(),
            netuid1,
            amount_to_be_staked,
            limit_stake,
            allow
        ));

        // Swap the complete origin relationship so the benchmark includes
        // its cleanup before the destination stake recreates it.
        let amount_swapped =
            Subtensor::<T>::get_stake_for_hotkey_and_coldkey_on_subnet(&hot, &coldkey, netuid1);
        let limit_swap = TaoBalance::ZERO;

        #[extrinsic_call]
        _(
            RawOrigin::Signed(coldkey.clone()),
            hot.clone(),
            netuid1,
            netuid2,
            amount_swapped,
            limit_swap,
            allow,
        );

        assert!(
            Subtensor::<T>::get_stake_for_hotkey_and_coldkey_on_subnet(&hot, &coldkey, netuid1,)
                .is_zero()
        );
    }

    #[benchmark]
    fn transfer_stake() {
        let coldkey: T::AccountId = whitelisted_caller();
        let dest: T::AccountId = account("B", 0, 2);
        let hot: T::AccountId = account("A", 0, 1);
        let netuid = NetUid::from(1);

        SubtokenEnabled::<T>::insert(netuid, true);
        Subtensor::<T>::init_new_network(netuid, 1);
        Subtensor::<T>::set_network_registration_allowed(netuid, true);

        let reg_fee = Subtensor::<T>::get_burn(netuid);
        let stake_tao = DefaultMinStake::<T>::get().saturating_mul(10.into());
        let deposit = reg_fee.saturating_mul(2.into()).saturating_add(stake_tao);
        add_balance_to_coldkey_account::<T>(&coldkey, deposit.into());
        add_lock::<T>(&coldkey, netuid);

        assert_ok!(Subtensor::<T>::burned_register(
            RawOrigin::Signed(coldkey.clone()).into(),
            netuid,
            hot.clone()
        ));

        set_reserves::<T>(netuid, deposit, AlphaBalance::from(deposit.to_u64()));
        TotalStake::<T>::set(deposit);

        assert_ok!(Subtensor::<T>::add_stake_limit(
            RawOrigin::Signed(coldkey.clone()).into(),
            hot.clone(),
            netuid,
            stake_tao,
            TaoBalance::MAX,
            false
        ));

        let alpha_to_transfer =
            Subtensor::<T>::get_stake_for_hotkey_and_coldkey_on_subnet(&hot, &coldkey, netuid);

        let _ = Subtensor::<T>::create_account_if_non_existent(&dest, &hot);

        #[extrinsic_call]
        _(
            RawOrigin::Signed(coldkey.clone()),
            dest.clone(),
            hot.clone(),
            netuid,
            netuid,
            alpha_to_transfer,
        );

        assert!(!StakingHotkeys::<T>::contains_key(&coldkey));
        assert!(StakingHotkeys::<T>::get(&dest).contains(&hot));
    }

    #[benchmark]
    fn transfer_stake_and_hotkey() {
        let coldkey: T::AccountId = whitelisted_caller();
        let dest: T::AccountId = account("B", 0, 2);
        let hot: T::AccountId = account("A", 0, 1);
        let dest_hot: T::AccountId = account("C", 0, 3);
        let netuid = NetUid::from(1);

        SubtokenEnabled::<T>::insert(netuid, true);
        Subtensor::<T>::init_new_network(netuid, 1);
        Subtensor::<T>::set_network_registration_allowed(netuid, true);

        let reg_fee = Subtensor::<T>::get_burn(netuid);
        let stake_tao = DefaultMinStake::<T>::get().saturating_mul(10.into());
        let deposit = reg_fee.saturating_mul(2.into()).saturating_add(stake_tao);
        add_balance_to_coldkey_account::<T>(&coldkey, deposit.into());
        add_lock::<T>(&coldkey, netuid);

        assert_ok!(Subtensor::<T>::burned_register(
            RawOrigin::Signed(coldkey.clone()).into(),
            netuid,
            hot.clone()
        ));

        set_reserves::<T>(netuid, deposit, AlphaBalance::from(deposit.to_u64()));
        TotalStake::<T>::set(deposit);

        assert_ok!(Subtensor::<T>::add_stake_limit(
            RawOrigin::Signed(coldkey.clone()).into(),
            hot.clone(),
            netuid,
            stake_tao,
            TaoBalance::MAX,
            false
        ));

        let alpha_to_transfer =
            Subtensor::<T>::get_stake_for_hotkey_and_coldkey_on_subnet(&hot, &coldkey, netuid);

        let _ = Subtensor::<T>::create_account_if_non_existent(&dest, &dest_hot);

        #[extrinsic_call]
        _(
            RawOrigin::Signed(coldkey.clone()),
            dest.clone(),
            hot.clone(),
            dest_hot.clone(),
            netuid,
            netuid,
            alpha_to_transfer,
        );

        assert!(!StakingHotkeys::<T>::contains_key(&coldkey));
        assert!(StakingHotkeys::<T>::get(&dest).contains(&dest_hot));
    }

    #[benchmark]
    fn add_collateral() {
        let coldkey: T::AccountId = whitelisted_caller();
        let hot: T::AccountId = account("A", 0, 1);
        let netuid = NetUid::from(1);

        SubtokenEnabled::<T>::insert(netuid, true);
        Subtensor::<T>::init_new_network(netuid, 1);
        Subtensor::<T>::set_network_registration_allowed(netuid, true);

        let reg_fee = Subtensor::<T>::get_burn(netuid);
        let collateral_alpha = AlphaBalance::from(u64::from(
            DefaultMinStake::<T>::get().saturating_mul(10.into()),
        ));
        let deposit = reg_fee
            .saturating_mul(2.into())
            .saturating_add(TaoBalance::from(collateral_alpha.to_u64()).saturating_mul(2.into()));
        add_balance_to_coldkey_account::<T>(&coldkey, deposit.into());
        add_lock::<T>(&coldkey, netuid);

        assert_ok!(Subtensor::<T>::burned_register(
            RawOrigin::Signed(coldkey.clone()).into(),
            netuid,
            hot.clone()
        ));

        set_reserves::<T>(netuid, deposit, AlphaBalance::from(deposit.to_u64()));
        TotalStake::<T>::set(deposit);
        // Moving price ≈ 1 so shortfall alpha maps 1:1 into TAO for the buy.
        SubnetMovingPrice::<T>::insert(netuid, I96F32::from_num(1));

        // Worst case: free stake covers only part of the target (lock-from-stake
        // + buy shortfall), and an existing entry must be merged.
        let already_locked = AlphaBalance::from(1_000u64);
        let free_partial = AlphaBalance::from(collateral_alpha.to_u64() / 2);
        Subtensor::<T>::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hot,
            &coldkey,
            netuid,
            free_partial.saturating_add(already_locked),
        );
        MinerCollateral::<T>::insert(
            (netuid, &hot, &coldkey),
            MinerCollateralState {
                locked: already_locked,
                drain_ratio: U64F64::saturating_from_num(1),
                min_locked: already_locked,
                earned: AlphaBalance::ZERO,
            },
        );
        ColdkeyMinerCollateral::<T>::insert(netuid, &coldkey, already_locked);
        ColdkeyCollateralHotkeys::<T>::mutate(netuid, &coldkey, |hotkeys| {
            let _ = hotkeys.try_push(hot.clone());
        });

        // Bound at max so the measured path still exercises the buy leg;
        // production callers pass spot × (1 + tolerance).
        let limit_price = T::SwapInterface::max_price();

        #[extrinsic_call]
        _(
            RawOrigin::Signed(coldkey.clone()),
            netuid,
            hot.clone(),
            collateral_alpha,
            limit_price,
        );
    }

    #[benchmark]
    fn set_min_collateral() {
        let coldkey: T::AccountId = whitelisted_caller();
        let hot: T::AccountId = account("A", 0, 1);
        let netuid = NetUid::from(1);

        SubtokenEnabled::<T>::insert(netuid, true);
        Subtensor::<T>::init_new_network(netuid, 1);
        let _ = Subtensor::<T>::create_account_if_non_existent(&coldkey, &hot);

        #[extrinsic_call]
        _(
            RawOrigin::Signed(coldkey.clone()),
            netuid,
            hot.clone(),
            AlphaBalance::from(1_000_000u64),
        );
    }

    #[benchmark]
    fn swap_stake() {
        let coldkey: T::AccountId = whitelisted_caller();
        let hot: T::AccountId = account("A", 0, 9);
        let netuid1 = NetUid::from(1);
        let netuid2 = NetUid::from(2);

        SubtokenEnabled::<T>::insert(netuid1, true);
        Subtensor::<T>::init_new_network(netuid1, 1);
        Subtensor::<T>::set_network_registration_allowed(netuid1, true);

        SubtokenEnabled::<T>::insert(netuid2, true);
        Subtensor::<T>::init_new_network(netuid2, 1);
        Subtensor::<T>::set_network_registration_allowed(netuid2, true);

        let reg_fee = Subtensor::<T>::get_burn(netuid1);
        let stake_tao = DefaultMinStake::<T>::get().saturating_mul(10.into());
        let deposit = reg_fee.saturating_mul(2.into()).saturating_add(stake_tao);
        add_balance_to_coldkey_account::<T>(&coldkey, deposit.into());
        add_lock::<T>(&coldkey, netuid1);
        add_lock::<T>(&coldkey, netuid2);

        assert_ok!(Subtensor::<T>::burned_register(
            RawOrigin::Signed(coldkey.clone()).into(),
            netuid1,
            hot.clone()
        ));

        set_reserves::<T>(netuid1, deposit, AlphaBalance::from(deposit.to_u64()));
        set_reserves::<T>(netuid2, deposit, AlphaBalance::from(deposit.to_u64()));
        TotalStake::<T>::set(deposit);

        assert_ok!(Subtensor::<T>::add_stake_limit(
            RawOrigin::Signed(coldkey.clone()).into(),
            hot.clone(),
            netuid1,
            stake_tao,
            TaoBalance::MAX,
            false
        ));

        let alpha_to_swap =
            Subtensor::<T>::get_stake_for_hotkey_and_coldkey_on_subnet(&hot, &coldkey, netuid1);

        #[extrinsic_call]
        _(
            RawOrigin::Signed(coldkey.clone()),
            hot.clone(),
            netuid1,
            netuid2,
            alpha_to_swap,
        );

        assert!(
            Subtensor::<T>::get_stake_for_hotkey_and_coldkey_on_subnet(&hot, &coldkey, netuid1,)
                .is_zero()
        );
    }

    #[benchmark]
    fn batch_commit_weights() {
        let hotkey: T::AccountId = whitelisted_caller();
        let netuid = NetUid::from(1);
        let count: usize = 3;
        let mut netuids: Vec<Compact<NetUid>> = Vec::new();
        let mut hashes: Vec<H256> = Vec::new();

        Subtensor::<T>::init_new_network(netuid, 1);
        Subtensor::<T>::set_network_registration_allowed(netuid, true);
        SubtokenEnabled::<T>::insert(netuid, true);
        Subtensor::<T>::set_weights_set_rate_limit(netuid, 0);

        Subtensor::<T>::set_burn(netuid, benchmark_registration_burn());
        seed_swap_reserves::<T>(netuid);
        fund_for_registration::<T>(netuid, &hotkey);

        assert_ok!(Subtensor::<T>::burned_register(
            RawOrigin::Signed(hotkey.clone()).into(),
            netuid,
            hotkey.clone()
        ));

        Subtensor::<T>::set_validator_permit_for_uid(netuid, 0, true);
        Subtensor::<T>::set_commit_reveal_weights_enabled(netuid, true);

        for i in 0..count {
            netuids.push(Compact(netuid));
            hashes.push(H256::repeat_byte(i as u8));
        }

        #[extrinsic_call]
        _(
            RawOrigin::Signed(hotkey.clone()),
            netuids.clone(),
            hashes.clone(),
        );
    }

    #[benchmark]
    fn batch_set_weights() {
        let hotkey: T::AccountId = whitelisted_caller();
        let netuid = NetUid::from(1);
        let version: u64 = 1;
        let entries: Vec<(Compact<u16>, Compact<u16>)> = vec![(Compact(0u16), Compact(0u16))];
        let netuids: Vec<Compact<NetUid>> = vec![Compact(netuid)];
        let weights: Vec<Vec<(Compact<u16>, Compact<u16>)>> = vec![entries.clone()];
        let keys: Vec<Compact<u64>> = vec![Compact(version)];

        Subtensor::<T>::init_new_network(netuid, 1);
        Subtensor::<T>::set_network_registration_allowed(netuid, true);
        SubtokenEnabled::<T>::insert(netuid, true);
        Subtensor::<T>::set_commit_reveal_weights_enabled(netuid, false);

        // Avoid any weights set rate-limit edge cases during benchmark setup.
        Subtensor::<T>::set_weights_set_rate_limit(netuid, 0);

        Subtensor::<T>::set_burn(netuid, benchmark_registration_burn());
        seed_swap_reserves::<T>(netuid);
        fund_for_registration::<T>(netuid, &hotkey);

        assert_ok!(Subtensor::<T>::burned_register(
            RawOrigin::Signed(hotkey.clone()).into(),
            netuid,
            hotkey.clone()
        ));

        // Batch set weights generally requires validator permit.
        Subtensor::<T>::set_validator_permit_for_uid(netuid, 0, true);

        #[extrinsic_call]
        _(
            RawOrigin::Signed(hotkey.clone()),
            netuids.clone(),
            weights.clone(),
            keys.clone(),
        );
    }

    #[benchmark]
    fn decrease_take() {
        let coldkey: T::AccountId = whitelisted_caller();
        let hotkey: T::AccountId = account("Alice", 0, 1);
        let min_take = Subtensor::<T>::get_min_delegate_take();
        let take = PerU16::from_parts(min_take);
        let current_take = PerU16::from_parts(min_take.saturating_add(1));

        Delegates::<T>::insert(&hotkey, current_take);
        Owner::<T>::insert(&hotkey, &coldkey);

        #[extrinsic_call]
        _(RawOrigin::Signed(coldkey.clone()), hotkey.clone(), take);
    }

    #[benchmark]
    fn increase_take() {
        let coldkey: T::AccountId = whitelisted_caller();
        let hotkey: T::AccountId = account("Alice", 0, 2);
        let take = PerU16::from_parts(150);

        Delegates::<T>::insert(&hotkey, PerU16::from_parts(100));
        Owner::<T>::insert(&hotkey, &coldkey);

        #[extrinsic_call]
        _(RawOrigin::Signed(coldkey.clone()), hotkey.clone(), take);
    }

    #[benchmark]
    fn register_network_with_identity() {
        let coldkey: T::AccountId = whitelisted_caller();
        let hotkey: T::AccountId = account("Alice", 0, 1);
        let identity: Option<SubnetIdentityOfV3> = None;

        Subtensor::<T>::set_network_registration_allowed(1.into(), true);
        Subtensor::<T>::set_network_rate_limit(1);
        let amount: u64 = 9_999_999_999_999;
        add_balance_to_coldkey_account::<T>(&coldkey, amount.into());

        #[extrinsic_call]
        _(
            RawOrigin::Signed(coldkey.clone()),
            hotkey.clone(),
            identity.clone(),
        );
    }

    #[benchmark]
    fn serve_axon_tls() {
        let caller: T::AccountId = whitelisted_caller();
        let netuid = NetUid::from(1);
        let version: u32 = 1;
        let ip: u128 = 0xC0A8_0001;
        let port: u16 = 30333;
        let ip_type: u8 = 4;
        let proto: u8 = 0;
        let p1: u8 = 0;
        let p2: u8 = 0;
        let cert: Vec<u8> = vec![];

        Subtensor::<T>::init_new_network(netuid, 1);
        Subtensor::<T>::set_network_registration_allowed(netuid, true);
        SubtokenEnabled::<T>::insert(netuid, true);

        Subtensor::<T>::set_burn(netuid, benchmark_registration_burn());
        seed_swap_reserves::<T>(netuid);
        fund_for_registration::<T>(netuid, &caller);

        assert_ok!(Subtensor::<T>::burned_register(
            RawOrigin::Signed(caller.clone()).into(),
            netuid,
            caller.clone()
        ));

        #[extrinsic_call]
        _(
            RawOrigin::Signed(caller.clone()),
            netuid,
            version,
            ip,
            port,
            ip_type,
            proto,
            p1,
            p2,
            cert.clone(),
        );
    }

    #[benchmark]
    fn set_identity() {
        let netuid = NetUid::from(1);
        let coldkey: T::AccountId = whitelisted_caller();
        let hotkey: T::AccountId = account("Alice", 0, 5);
        let name = b"n".to_vec();
        let url = vec![];
        let repo = vec![];
        let img = vec![];
        let disc = vec![];
        let descr = vec![];
        let add = vec![];

        let _ = Subtensor::<T>::create_account_if_non_existent(&coldkey, &hotkey);
        Subtensor::<T>::init_new_network(netuid, 1);
        Subtensor::<T>::set_network_registration_allowed(netuid, true);
        SubtokenEnabled::<T>::insert(netuid, true);
        Subtensor::<T>::set_burn(netuid, benchmark_registration_burn());
        seed_swap_reserves::<T>(netuid);

        let deposit: u64 = 1_000_000_000u64.saturating_mul(2);
        add_balance_to_coldkey_account::<T>(&coldkey, deposit.into());

        assert_ok!(Subtensor::<T>::burned_register(
            RawOrigin::Signed(coldkey.clone()).into(),
            netuid,
            hotkey.clone()
        ));

        #[extrinsic_call]
        _(
            RawOrigin::Signed(coldkey.clone()),
            name,
            url,
            repo,
            img,
            disc,
            descr,
            add,
        );
    }

    #[benchmark]
    fn set_subnet_identity() {
        let coldkey: T::AccountId = whitelisted_caller();
        let netuid = NetUid::from(1);
        let name = b"n".to_vec();
        let repo = vec![];
        let contact = vec![];
        let url = vec![];
        let disc = vec![];
        let descr = vec![];
        let logo_url = vec![];
        let add = vec![];

        Subtensor::<T>::init_new_network(netuid, 1);
        SubnetOwner::<T>::insert(netuid, coldkey.clone());
        SubtokenEnabled::<T>::insert(netuid, true);

        #[extrinsic_call]
        _(
            RawOrigin::Signed(coldkey.clone()),
            netuid,
            name,
            repo,
            contact,
            url,
            disc,
            descr,
            logo_url,
            add,
        );
    }

    #[benchmark]
    fn swap_hotkey() {
        let coldkey: T::AccountId = whitelisted_caller();
        let old: T::AccountId = account("A", 0, 7);
        let new: T::AccountId = account("B", 0, 8);

        const INCIDENT_SUBNETS: u16 = 16;
        const INCIDENT_STAKE_POSITIONS: u32 = 1_273;

        let alpha_amount = AlphaBalance::from(1_000_000_u64);
        let subnet_alpha = AlphaBalance::from(1_000_000_000_000_u64);

        // Populate the reduced topology and make the old hotkey a member
        // everywhere so the benchmark includes the per-subnet metadata path.
        for i in 1..=INCIDENT_SUBNETS {
            let netuid = NetUid::from(i);
            Subtensor::<T>::init_new_network(netuid, 1);
            Subtensor::<T>::set_max_allowed_uids(netuid, 1);
            SubtokenEnabled::<T>::insert(netuid, true);
            seed_swap_reserves::<T>(netuid);
            SubnetAlphaOut::<T>::insert(netuid, subnet_alpha);
            Subtensor::<T>::append_neuron(netuid, &old, 0);
        }

        // Use distinct coldkeys so execution performs the reduced number
        // of actual position migrations and StakingHotkeys index rewrites. The
        // positions are spread evenly over all active subnets.
        for i in 0..INCIDENT_STAKE_POSITIONS {
            let staker: T::AccountId = account("stake", i, 9);
            let netuid = NetUid::from(((i % u32::from(INCIDENT_SUBNETS)).saturating_add(1)) as u16);
            Subtensor::<T>::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &old,
                &staker,
                netuid,
                alpha_amount,
            );
        }

        Owner::<T>::insert(&old, &coldkey);
        let ed = <T as pallet_balances::Config>::ExistentialDeposit::get();
        let cost = Subtensor::<T>::get_key_swap_cost();
        add_balance_to_coldkey_account::<T>(&coldkey, cost + ed);

        #[extrinsic_call]
        _(RawOrigin::Signed(coldkey.clone()), old, new, None);
    }

    #[benchmark]
    fn try_associate_hotkey() {
        let coldkey: T::AccountId = whitelisted_caller();
        let hot: T::AccountId = account("A", 0, 1);

        #[extrinsic_call]
        _(RawOrigin::Signed(coldkey.clone()), hot);
    }

    #[benchmark]
    fn unstake_all() {
        let coldkey: T::AccountId = whitelisted_caller();
        let hotkey: T::AccountId = account("A", 0, 14);
        let _ = Subtensor::<T>::create_account_if_non_existent(&coldkey, &hotkey);

        #[extrinsic_call]
        _(RawOrigin::Signed(coldkey.clone()), hotkey);
    }

    #[benchmark]
    fn unstake_all_alpha() {
        let netuid = NetUid::from(1);
        let tempo: u16 = 1;
        let seed: u32 = 1;

        Subtensor::<T>::init_new_network(netuid, tempo);
        Subtensor::<T>::set_network_registration_allowed(netuid, true);
        SubtokenEnabled::<T>::insert(netuid, true);

        Subtensor::<T>::set_max_allowed_uids(netuid, 4096);
        assert_eq!(Subtensor::<T>::get_max_allowed_uids(netuid), 4096);

        let coldkey: T::AccountId = account("Test", 0, seed);
        let hotkey: T::AccountId = account("Alice", 0, seed);
        Subtensor::<T>::set_burn(netuid, benchmark_registration_burn());

        set_reserves::<T>(
            netuid,
            TaoBalance::from(150_000_000_000_u64),
            AlphaBalance::from(100_000_000_000_u64),
        );

        // Registration now requires keep-alive coverage of the burn; fund
        // above burn + ED rather than a flat token amount.
        fund_for_registration::<T>(netuid, &coldkey);

        assert_ok!(Subtensor::<T>::burned_register(
            RawOrigin::Signed(coldkey.clone()).into(),
            netuid,
            hotkey.clone()
        ));

        let staked_amt = TaoBalance::from(100_000_000_000_u64);
        add_balance_to_coldkey_account::<T>(&coldkey.clone(), staked_amt);

        assert_ok!(Subtensor::<T>::add_stake(
            RawOrigin::Signed(coldkey.clone()).into(),
            hotkey.clone(),
            netuid,
            staked_amt
        ));

        #[extrinsic_call]
        _(RawOrigin::Signed(coldkey), hotkey);
    }

    #[benchmark]
    fn remove_stake_full_limit() {
        let netuid = NetUid::from(1);
        let tempo: u16 = 1;
        let seed: u32 = 1;

        Subtensor::<T>::increase_total_stake(1_000_000_000_000_u64.into());

        Subtensor::<T>::init_new_network(netuid, tempo);
        Subtensor::<T>::set_network_registration_allowed(netuid, true);
        SubtokenEnabled::<T>::insert(netuid, true);

        Subtensor::<T>::set_max_allowed_uids(netuid, 4096);
        assert_eq!(Subtensor::<T>::get_max_allowed_uids(netuid), 4096);

        let coldkey: T::AccountId = account("Test", 0, seed);
        let hotkey: T::AccountId = account("Alice", 0, seed);
        Subtensor::<T>::set_burn(netuid, benchmark_registration_burn());

        let tao_reserve = TaoBalance::from(1_000_000_000_000_u64);
        let alpha_in = AlphaBalance::from(100_000_000_000_000_u64);
        set_reserves::<T>(netuid, tao_reserve, alpha_in);

        // Registration now requires keep-alive coverage of the burn.
        fund_for_registration::<T>(netuid, &coldkey);
        add_lock::<T>(&coldkey, netuid);

        assert_ok!(Subtensor::<T>::burned_register(
            RawOrigin::Signed(coldkey.clone()).into(),
            netuid,
            hotkey.clone()
        ));

        // Read current price and set limit price 50% lower, which is not getting hit
        // by swapping 1 TAO
        let current_price = T::SwapInterface::current_alpha_price(netuid);
        let limit = current_price
            .saturating_mul(U64F64::saturating_from_num(500_000_000))
            .saturating_to_num::<u64>()
            .into();
        let staked_amt = TaoBalance::from(1_000_000_000_u64);
        add_balance_to_coldkey_account::<T>(&coldkey.clone(), staked_amt);

        assert_ok!(Subtensor::<T>::add_stake(
            RawOrigin::Signed(coldkey.clone()).into(),
            hotkey.clone(),
            netuid,
            staked_amt
        ));

        #[extrinsic_call]
        _(
            RawOrigin::Signed(coldkey.clone()),
            hotkey.clone(),
            netuid,
            Some(limit),
        );

        assert!(!StakingHotkeys::<T>::contains_key(&coldkey));
    }

    #[benchmark]
    fn register_leased_network(k: Linear<2, { T::MaxContributors::get() }>) {
        // Setup a crowdloan
        let crowdloan_id = 0;
        let beneficiary: T::AccountId = whitelisted_caller();
        let deposit = TaoBalance::from(20_000_000_000_u64); // 20 TAO
        let now = frame_system::Pallet::<T>::block_number(); // not really important here
        let end = now + T::MaximumBlockDuration::get();
        let cap = TaoBalance::from(2_000_000_000_000_u64); // 2000 TAO

        let funds_account: T::AccountId = account("funds", 0, 0);
        add_balance_to_coldkey_account::<T>(&funds_account, cap.into());

        pallet_crowdloan::Crowdloans::<T>::insert(
            crowdloan_id,
            pallet_crowdloan::CrowdloanInfo {
                creator: beneficiary.clone(),
                deposit,
                min_contribution: 0.into(),
                end,
                cap,
                raised: cap,
                finalized: false,
                funds_account: funds_account.clone(),
                call: None,
                target_address: None,
                contributors_count: T::MaxContributors::get(),
            },
        );

        frame_system::Pallet::<T>::set_block_number(end);

        pallet_crowdloan::Contributions::<T>::insert(crowdloan_id, &beneficiary, deposit);

        let contributors = k - 1;
        let amount = (cap - deposit) / TaoBalance::from(contributors);
        for i in 0..contributors {
            let contributor = account::<T::AccountId>("contributor", i.try_into().unwrap(), 0);
            pallet_crowdloan::Contributions::<T>::insert(crowdloan_id, contributor, amount);
        }

        pallet_crowdloan::CurrentCrowdloanId::<T>::set(Some(0));

        let emissions_share = Percent::from_percent(30);
        #[extrinsic_call]
        _(
            RawOrigin::Signed(beneficiary.clone()),
            emissions_share,
            None,
        );

        let lease_id = 0;
        let lease = SubnetLeases::<T>::get(lease_id).unwrap();
        assert_eq!(lease.beneficiary, beneficiary);
        assert_eq!(lease.emissions_share, emissions_share);
        assert_eq!(lease.end_block, None);

        assert!(SubnetMechanism::<T>::contains_key(lease.netuid));
    }

    #[benchmark]
    fn terminate_lease(k: Linear<2, { T::MaxContributors::get() }>) {
        let crowdloan_id = 0;
        let beneficiary: T::AccountId = whitelisted_caller();
        let deposit = TaoBalance::from(20_000_000_000_u64); // 20 TAO
        let now = frame_system::Pallet::<T>::block_number();
        let crowdloan_end = now + T::MaximumBlockDuration::get();
        let cap = TaoBalance::from(2_000_000_000_000_u64); // 2000 TAO

        let funds_account: T::AccountId = account("funds", 0, 0);
        add_balance_to_coldkey_account::<T>(&funds_account, cap);

        pallet_crowdloan::Crowdloans::<T>::insert(
            crowdloan_id,
            pallet_crowdloan::CrowdloanInfo {
                creator: beneficiary.clone(),
                deposit,
                min_contribution: 0.into(),
                end: crowdloan_end,
                cap,
                raised: cap,
                finalized: false,
                funds_account: funds_account.clone(),
                call: None,
                target_address: None,
                contributors_count: T::MaxContributors::get(),
            },
        );

        frame_system::Pallet::<T>::set_block_number(crowdloan_end);

        pallet_crowdloan::Contributions::<T>::insert(crowdloan_id, &beneficiary, deposit);

        let contributors = k - 1;
        let amount = (cap - deposit) / TaoBalance::from(contributors);
        for i in 0..contributors {
            let contributor = account::<T::AccountId>("contributor", i.try_into().unwrap(), 0);
            pallet_crowdloan::Contributions::<T>::insert(crowdloan_id, contributor, amount);
        }

        pallet_crowdloan::CurrentCrowdloanId::<T>::set(Some(0));

        let emissions_share = Percent::from_percent(30);
        let lease_end = crowdloan_end + 1000u32.into();
        assert_ok!(Subtensor::<T>::register_leased_network(
            RawOrigin::Signed(beneficiary.clone()).into(),
            emissions_share,
            Some(lease_end),
        ));

        frame_system::Pallet::<T>::set_block_number(lease_end);

        let lease_id = 0;
        let lease = SubnetLeases::<T>::get(0).unwrap();
        let hotkey = account::<T::AccountId>("beneficiary_hotkey", 0, 0);
        let _ = Subtensor::<T>::create_account_if_non_existent(&beneficiary, &hotkey);

        #[extrinsic_call]
        _(
            RawOrigin::Signed(beneficiary.clone()),
            lease_id,
            hotkey.clone(),
        );

        assert_eq!(SubnetOwner::<T>::get(lease.netuid), beneficiary);
        assert_eq!(SubnetOwnerHotkey::<T>::get(lease.netuid), hotkey);

        assert_eq!(SubnetLeases::<T>::get(lease_id), None);
        assert!(!SubnetLeaseShares::<T>::contains_prefix(lease_id));
        assert!(!AccumulatedLeaseDividends::<T>::contains_key(lease_id));
    }

    #[benchmark]
    fn update_symbol() {
        let coldkey: T::AccountId = whitelisted_caller();
        let netuid = NetUid::from(1);
        let tempo: u16 = 1;
        Subtensor::<T>::init_new_network(netuid, tempo);
        SubnetOwner::<T>::insert(netuid, coldkey.clone());

        let new_symbol = Subtensor::<T>::get_symbol_for_subnet(NetUid::from(42));

        #[extrinsic_call]
        _(RawOrigin::Signed(coldkey), netuid, new_symbol.clone());

        assert_eq!(TokenSymbol::<T>::get(netuid), new_symbol);
    }

    #[benchmark]
    fn commit_timelocked_weights() {
        let hotkey: T::AccountId = whitelisted_caller();
        let netuid = NetUid::from(1);
        let vec_commit: Vec<u8> = vec![0; MAX_CRV3_COMMIT_SIZE_BYTES as usize];
        let commit: BoundedVec<_, _> = vec_commit.try_into().unwrap();
        let round: u64 = 0;

        Subtensor::<T>::init_new_network(netuid, 1);
        Subtensor::<T>::set_network_registration_allowed(netuid, true);
        SubtokenEnabled::<T>::insert(netuid, true);

        Subtensor::<T>::set_burn(netuid, benchmark_registration_burn());
        seed_swap_reserves::<T>(netuid);
        fund_for_registration::<T>(netuid, &hotkey);

        assert_ok!(Subtensor::<T>::burned_register(
            RawOrigin::Signed(hotkey.clone()).into(),
            netuid,
            hotkey.clone()
        ));

        // Ensure caller is allowed to commit (common requirement for weights ops).
        Subtensor::<T>::set_validator_permit_for_uid(netuid, 0, true);

        Subtensor::<T>::set_commit_reveal_weights_enabled(netuid, true);
        WeightsSetRateLimit::<T>::set(netuid, 0);

        #[extrinsic_call]
        _(
            RawOrigin::Signed(hotkey.clone()),
            netuid,
            commit.clone(),
            round,
            Subtensor::<T>::get_commit_reveal_weights_version(),
        );
    }

    #[benchmark]
    fn set_coldkey_auto_stake_hotkey() {
        let coldkey: T::AccountId = whitelisted_caller();
        let netuid = NetUid::from(1);
        let hotkey: T::AccountId = account("A", 0, 1);

        SubtokenEnabled::<T>::insert(netuid, true);
        Subtensor::<T>::init_new_network(netuid, 1);
        Subtensor::<T>::set_network_registration_allowed(netuid, true);

        let amount = 900_000_000_000u64;
        add_balance_to_coldkey_account::<T>(&coldkey.clone(), amount.into());

        assert_ok!(Subtensor::<T>::burned_register(
            RawOrigin::Signed(coldkey.clone()).into(),
            netuid,
            hotkey.clone()
        ));

        #[extrinsic_call]
        _(RawOrigin::Signed(coldkey.clone()), netuid, hotkey.clone());
    }

    #[benchmark]
    fn claim_root(h: Linear<1, { crate::MAX_ROOT_CLAIM_WORK }>) {
        // Coldkey-wide claim: `h` validator hotkeys, one holding each. `subnets` is ignored.
        let coldkey: T::AccountId = whitelisted_caller();
        let owner_coldkey: T::AccountId = account("claim_owner_cold", 0, 0);
        let owner_hotkey: T::AccountId = account("claim_owner_hot", 0, 1);
        let netuid = Subtensor::<T>::get_next_netuid();

        let lock_cost = Subtensor::<T>::get_network_lock_cost();
        add_balance_to_coldkey_account::<T>(&owner_coldkey, lock_cost.into());

        assert_ok!(Subtensor::<T>::register_network(
            RawOrigin::Signed(owner_coldkey).into(),
            owner_hotkey
        ));
        SubtokenEnabled::<T>::insert(netuid, true);
        Subtensor::<T>::set_network_registration_allowed(netuid, true);
        NetworkRegistrationAllowed::<T>::insert(netuid, true);
        FirstEmissionBlockNumber::<T>::insert(netuid, 0);
        SubnetMechanism::<T>::insert(netuid, 1);
        SubnetworkN::<T>::insert(netuid, 1);
        Subtensor::<T>::set_tao_weight(u64::MAX);
        set_reserves::<T>(
            netuid,
            TaoBalance::from(100_000_000_000_000_u64),
            AlphaBalance::from(100_000_000_000_000_u64),
        );
        RootClaimableThreshold::<T>::insert(NetUid::ROOT, I96F32::from_num(0));

        let escrow = Subtensor::<T>::get_beta_escrow_account_id();
        let holding_alpha = AlphaBalance::from(1_000_000_u64);
        for i in 0..h {
            let hotkey: T::AccountId = account("claim_hot", i, 1);
            Subtensor::<T>::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &coldkey,
                NetUid::ROOT,
                AlphaBalance::from(1_u64),
            );
            Subtensor::<T>::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &escrow,
                netuid,
                holding_alpha,
            );
            BasketShares::<T>::insert(&hotkey, 1_u64);
            BasketRate::<T>::insert(&hotkey, I96F32::from_num(1));
        }

        let subnets = sp_std::collections::btree_set::BTreeSet::from([NetUid::ROOT]);
        #[extrinsic_call]
        _(RawOrigin::Signed(coldkey.clone()), subnets);

        let first_hotkey: T::AccountId = account("claim_hot", 0, 1);
        let last_hotkey: T::AccountId = account("claim_hot", h.saturating_sub(1), 1);
        assert_eq!(BasketShares::<T>::get(first_hotkey), 0);
        assert_eq!(BasketShares::<T>::get(last_hotkey), 0);
    }

    #[benchmark]
    fn claim_root_scan(h: Linear<1, { crate::MAX_ROOT_CLAIM_WORK }>) {
        // Scan-only claim: `h` validator hotkeys, one holding each, where the holding is
        // above the dust threshold (no consolidation) but the claimant's marked payout is
        // below the claim threshold, so every fund is valued (one sim-swap per row) and
        // skipped without redeeming. Measures the per-row scan cost charged for holdings
        // a claim walks but does not touch.
        let coldkey: T::AccountId = whitelisted_caller();
        let owner_coldkey: T::AccountId = account("scan_owner_cold", 0, 0);
        let owner_hotkey: T::AccountId = account("scan_owner_hot", 0, 1);
        let netuid = Subtensor::<T>::get_next_netuid();

        let lock_cost = Subtensor::<T>::get_network_lock_cost();
        add_balance_to_coldkey_account::<T>(&owner_coldkey, lock_cost.into());

        assert_ok!(Subtensor::<T>::register_network(
            RawOrigin::Signed(owner_coldkey).into(),
            owner_hotkey
        ));
        SubtokenEnabled::<T>::insert(netuid, true);
        SubnetMechanism::<T>::insert(netuid, 1);
        set_reserves::<T>(
            netuid,
            TaoBalance::from(100_000_000_000_000_u64),
            AlphaBalance::from(100_000_000_000_000_u64),
        );
        RootClaimableThreshold::<T>::insert(NetUid::ROOT, I96F32::from_num(500_000));

        let escrow = Subtensor::<T>::get_beta_escrow_account_id();
        // Well above the 500k-rao dust threshold, so consolidation leaves the row alone.
        let holding_alpha = AlphaBalance::from(100_000_000_u64);
        let mut hotkeys: Vec<T::AccountId> = Vec::new();
        for i in 0..h {
            let hotkey: T::AccountId = account("scan_hot", i, 1);
            Subtensor::<T>::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &coldkey,
                NetUid::ROOT,
                AlphaBalance::from(1_u64),
            );
            Subtensor::<T>::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &escrow,
                netuid,
                holding_alpha,
            );
            // Outstanding shares dwarf the claimant's single owed share, so the marked
            // payout floors below the threshold and the claim no-ops after the scan.
            BasketShares::<T>::insert(&hotkey, 1_000_000_000_u64);
            BasketRate::<T>::insert(&hotkey, I96F32::from_num(1));
            hotkeys.push(hotkey);
        }

        #[block]
        {
            let outcome = Subtensor::<T>::do_root_claim(coldkey.clone(), hotkeys.clone())
                .expect("scan claim must succeed");
            assert!(outcome.tao == 0, "scan benchmark must not redeem");
        }

        let first_hotkey: T::AccountId = account("scan_hot", 0, 1);
        assert_eq!(BasketShares::<T>::get(first_hotkey), 1_000_000_000_u64);
    }

    #[benchmark]
    fn sudo_set_root_claim_threshold() {
        #[extrinsic_call]
        _(RawOrigin::Root, NetUid::ROOT, 100);
    }

    #[benchmark]
    fn set_auto_parent_delegation_enabled() {
        let seed: u32 = 1;
        let coldkey: T::AccountId = account("Test", 0, seed);
        let hotkey: T::AccountId = account("Alice", 0, seed);

        Subtensor::<T>::init_new_network(NetUid::ROOT, 1);
        Subtensor::<T>::set_network_registration_allowed(NetUid::ROOT, true);
        FirstEmissionBlockNumber::<T>::insert(NetUid::ROOT, 1);
        SubtokenEnabled::<T>::insert(NetUid::ROOT, true);

        fund_for_registration::<T>(NetUid::ROOT, &coldkey);
        let _ = Subtensor::<T>::do_root_register(
            RawOrigin::Signed(coldkey.clone()).into(),
            hotkey.clone(),
        );

        #[extrinsic_call]
        _(RawOrigin::Signed(coldkey.clone()), hotkey, true);
    }

    #[benchmark]
    fn add_stake_burn() {
        let netuid = NetUid::from(1);
        let tempo: u16 = 1;
        let seed: u32 = 1;

        Subtensor::<T>::init_new_network(netuid, tempo);
        SubtokenEnabled::<T>::insert(netuid, true);
        Subtensor::<T>::set_burn(netuid, benchmark_registration_burn());
        Subtensor::<T>::set_network_registration_allowed(netuid, true);
        Subtensor::<T>::set_max_allowed_uids(netuid, 4096);

        let coldkey: T::AccountId = account("Test", 0, seed);
        let hotkey: T::AccountId = account("Alice", 0, seed);

        SubnetOwner::<T>::set(netuid, coldkey.clone());

        let balance_update = TaoBalance::from(900_000_000_000_u64);
        let limit = TaoBalance::from(6_000_000_000_u64);
        let amount = TaoBalance::from(44_000_000_000_u64);
        add_balance_to_coldkey_account::<T>(&coldkey.clone(), balance_update);
        add_lock::<T>(&coldkey, netuid);

        let tao_reserve = TaoBalance::from(150_000_000_000_u64);
        let alpha_in = AlphaBalance::from(100_000_000_000_u64);
        set_reserves::<T>(netuid, tao_reserve, alpha_in);

        assert_ok!(Subtensor::<T>::burned_register(
            RawOrigin::Signed(coldkey.clone()).into(),
            netuid,
            hotkey.clone()
        ));

        #[extrinsic_call]
        _(
            RawOrigin::Signed(coldkey.clone()),
            hotkey,
            netuid,
            amount,
            Some(limit),
        );
    }

    #[benchmark]
    fn set_pending_childkey_cooldown() {
        let cooldown: u64 = 7200;

        #[extrinsic_call]
        _(RawOrigin::Root, cooldown);

        assert_eq!(PendingChildKeyCooldown::<T>::get(), cooldown);
    }

    #[benchmark]
    fn lock_stake() {
        let netuid = NetUid::from(1);
        let tempo: u16 = 1;

        Subtensor::<T>::init_new_network(netuid, tempo);
        SubtokenEnabled::<T>::insert(netuid, true);
        Subtensor::<T>::set_burn(netuid, benchmark_registration_burn());
        Subtensor::<T>::set_network_registration_allowed(netuid, true);
        Subtensor::<T>::set_max_allowed_uids(netuid, 4096);

        let seed: u32 = 1;
        let coldkey: T::AccountId = account("Test", 0, seed);
        let hotkey: T::AccountId = account("Alice", 0, seed);
        let total_stake = TaoBalance::from(1_000_000_000);
        let amount = AlphaBalance::from(60_000_000);

        seed_swap_reserves::<T>(netuid);
        let burn = Subtensor::<T>::get_burn(netuid);
        add_balance_to_coldkey_account::<T>(
            &coldkey,
            total_stake
                .saturating_mul(2.into())
                .saturating_add(burn.saturating_mul(2.into()))
                .into(),
        );

        assert_ok!(Subtensor::<T>::burned_register(
            RawOrigin::Signed(coldkey.clone()).into(),
            netuid,
            hotkey.clone()
        ));

        assert_ok!(Subtensor::<T>::add_stake(
            RawOrigin::Signed(coldkey.clone()).into(),
            hotkey.clone(),
            netuid,
            total_stake
        ));

        #[extrinsic_call]
        _(
            RawOrigin::Signed(coldkey.clone()),
            hotkey.clone(),
            netuid,
            amount,
        );
    }

    #[benchmark]
    fn move_lock() {
        let netuid = NetUid::from(1);
        let tempo: u16 = 1;

        Subtensor::<T>::init_new_network(netuid, tempo);
        SubtokenEnabled::<T>::insert(netuid, true);
        Subtensor::<T>::set_burn(netuid, benchmark_registration_burn());
        Subtensor::<T>::set_network_registration_allowed(netuid, true);
        Subtensor::<T>::set_max_allowed_uids(netuid, 4096);

        let seed: u32 = 1;
        let coldkey: T::AccountId = account("Test", 0, seed);
        let hotkey: T::AccountId = account("Alice", 0, seed);
        let hotkey_dest: T::AccountId = account("Bob", 0, seed);
        let total_stake = TaoBalance::from(1_000_000_000);
        let amount = AlphaBalance::from(60_000_000);

        seed_swap_reserves::<T>(netuid);
        let burn = Subtensor::<T>::get_burn(netuid);
        add_balance_to_coldkey_account::<T>(
            &coldkey,
            total_stake
                .saturating_mul(2.into())
                .saturating_add(burn.saturating_mul(2.into()))
                .into(),
        );

        assert_ok!(Subtensor::<T>::burned_register(
            RawOrigin::Signed(coldkey.clone()).into(),
            netuid,
            hotkey.clone()
        ));

        assert_ok!(Subtensor::<T>::burned_register(
            RawOrigin::Signed(coldkey.clone()).into(),
            netuid,
            hotkey_dest.clone()
        ));

        assert_ok!(Subtensor::<T>::add_stake(
            RawOrigin::Signed(coldkey.clone()).into(),
            hotkey.clone(),
            netuid,
            total_stake
        ));

        assert_ok!(Subtensor::<T>::do_lock_stake(
            &coldkey, netuid, &hotkey, amount,
        ));

        #[extrinsic_call]
        _(
            RawOrigin::Signed(coldkey.clone()),
            hotkey_dest.clone(),
            netuid,
        );

        // Lock moving temporarily disabled
        assert!(
            Lock::<T>::iter_prefix((coldkey, netuid))
                .any(|(locked_hotkey, _)| locked_hotkey == hotkey_dest)
        );
    }

    #[benchmark]
    fn associate_evm_key() {
        let netuid = NetUid::from(1);
        let tempo: u16 = 1;

        let coldkey: T::AccountId = account("Test", 0, 1);
        let hotkey: T::AccountId = account("Alice", 0, 1);

        Subtensor::<T>::init_new_network(netuid, tempo);
        SubtokenEnabled::<T>::insert(netuid, true);
        Subtensor::<T>::set_network_registration_allowed(netuid, true);
        Subtensor::<T>::set_max_allowed_uids(netuid, 4096);
        Subtensor::<T>::set_burn(netuid, benchmark_registration_burn());

        seed_swap_reserves::<T>(netuid);
        fund_for_registration::<T>(netuid, &coldkey);

        assert_ok!(Subtensor::<T>::burned_register(
            RawOrigin::Signed(coldkey.clone()).into(),
            netuid,
            hotkey.clone()
        ));

        let uid = match Subtensor::<T>::get_uid_for_net_and_hotkey(netuid, &hotkey) {
            Ok(uid) => uid,
            Err(_) => panic!("registered benchmark hotkey must have a uid"),
        };

        // No existing association means `block_associated` is treated as 0.
        // Move the block forward enough to satisfy:
        // now - 0 >= T::EvmKeyAssociateRateLimit::get()
        let benchmark_block_number = T::EvmKeyAssociateRateLimit::get().saturating_add(1);

        let benchmark_block: BlockNumberFor<T> = match benchmark_block_number.try_into() {
            Ok(benchmark_block) => benchmark_block,
            Err(_) => panic!("benchmark block number must fit into BlockNumberFor<T>"),
        };

        frame_system::Pallet::<T>::set_block_number(benchmark_block);

        let block_number = Subtensor::<T>::get_current_block_as_u64();

        let evm_secret_key = benchmark_evm_secret_key();
        let evm_key = evm_key_from_secret_key(&evm_secret_key);

        let signature =
            signature_for_associate_evm_key::<T>(&hotkey, block_number, &evm_secret_key);

        #[extrinsic_call]
        _(
            RawOrigin::Signed(hotkey.clone()),
            netuid,
            evm_key,
            block_number,
            signature,
        );

        assert_eq!(
            AssociatedEvmAddress::<T>::get(netuid, uid),
            Some((evm_key, block_number))
        );
        assert_eq!(
            AssociatedUidsByEvmAddress::<T>::get(netuid, evm_key).into_inner(),
            vec![(uid, block_number)]
        );
    }

    #[benchmark]
    fn trigger_epoch() {
        let netuid = NetUid::from(1);
        let coldkey: T::AccountId = account("Owner", 0, 1);

        Subtensor::<T>::init_new_network(netuid, 1u16);
        SubnetOwner::<T>::insert(netuid, coldkey.clone());
        SubtokenEnabled::<T>::insert(netuid, true);
        Subtensor::<T>::set_commit_reveal_weights_enabled(netuid, false);
        Subtensor::<T>::set_admin_freeze_window(0);

        #[extrinsic_call]
        _(RawOrigin::Signed(coldkey.clone()), netuid);
    }

    #[benchmark]
    fn check_coldkey_swap_extension() {
        let coldkey: T::AccountId = account("coldkey", 0, 1);
        let new_coldkey: T::AccountId = account("new_coldkey", 0, 1);
        let hotkey: T::AccountId = account("hotkey", 0, 1);
        let new_coldkey_hash: T::Hash = <T as frame_system::Config>::Hashing::hash_of(&new_coldkey);
        let now = frame_system::Pallet::<T>::block_number();
        let call = runtime_call::<T>(Call::<T>::register_network { hotkey });

        ColdkeySwapAnnouncements::<T>::insert(&coldkey, (now, new_coldkey_hash));
        ColdkeySwapDisputes::<T>::insert(&coldkey, now);

        #[block]
        {
            assert_eq!(
                CheckColdkeySwap::<T>::check(&coldkey, &call),
                Err(Error::<T>::ColdkeySwapDisputed)
            );
        }
    }

    #[benchmark]
    fn check_weights_extension() {
        let netuid = NetUid::from(1);
        let hotkey: T::AccountId = account("hotkey", 0, 1);
        let netuid_index = NetUidStorageIndex::from(netuid);
        let uids: Vec<u16> = vec![0];
        let values: Vec<u16> = vec![10];
        let salt: Vec<u16> = vec![8];
        let version_key = 0_u64;

        setup_extension_neuron::<T>(netuid, &hotkey);
        Subtensor::<T>::set_stake_threshold(0);

        let commit_hash = Subtensor::<T>::get_commit_hash(
            &hotkey,
            netuid_index,
            &uids,
            &values,
            &salt,
            version_key,
        );
        let mut commits = VecDeque::new();
        for i in 0..9 {
            commits.push_back((H256::repeat_byte(i + 1), 0, 0, 0));
        }
        commits.push_back((commit_hash, 0, 0, 0));
        WeightCommits::<T>::insert(netuid_index, &hotkey, commits);

        let reveal_period = Subtensor::<T>::get_reveal_period(netuid);
        SubnetEpochIndex::<T>::insert(netuid, reveal_period);

        let call = Call::<T>::reveal_weights {
            netuid,
            uids,
            values,
            salt,
            version_key,
        };

        #[block]
        {
            assert_ok!(CheckWeights::<T>::check(&hotkey, &call));
        }
    }

    #[benchmark]
    fn check_rate_limits_extension() {
        let netuid = NetUid::from(1);
        let hotkey: T::AccountId = account("hotkey", 0, 1);
        let netuid_index = NetUidStorageIndex::from(netuid);
        let call = Call::<T>::set_weights {
            netuid,
            dests: vec![0],
            weights: vec![1],
            version_key: 0,
        };

        setup_extension_neuron::<T>(netuid, &hotkey);
        Subtensor::<T>::set_commit_reveal_weights_enabled(netuid, false);
        Subtensor::<T>::set_weights_set_rate_limit(netuid, 1);
        Subtensor::<T>::set_last_update_for_uid(netuid_index, 0, 1);
        set_benchmark_block_number::<T>(3);

        #[block]
        {
            assert_ok!(CheckRateLimits::<T>::check(&hotkey, &call));
        }
    }

    #[benchmark]
    fn check_delegate_take_extension() {
        let coldkey: T::AccountId = account("coldkey", 0, 1);
        let hotkey: T::AccountId = account("hotkey", 0, 1);
        let call = Call::<T>::increase_take {
            hotkey: hotkey.clone(),
            take: PerU16::from_parts(Subtensor::<T>::get_max_delegate_take()),
        };

        Owner::<T>::insert(&hotkey, &coldkey);

        #[block]
        {
            assert_ok!(CheckDelegateTake::<T>::check(&coldkey, &call));
        }
    }

    #[benchmark]
    fn check_serving_endpoints_extension() {
        let hotkey: T::AccountId = account("hotkey", 0, 1);
        let netuid = NetUid::from(1);
        let call = Call::<T>::serve_axon {
            netuid,
            version: 1,
            ip: u128::from(u32::from_be_bytes([8, 8, 8, 8])),
            port: 1,
            ip_type: 4,
            protocol: 0,
            placeholder1: 0,
            placeholder2: 0,
        };

        Uids::<T>::insert(netuid, &hotkey, 0);

        #[block]
        {
            assert_ok!(CheckServingEndpoints::<T>::check(&hotkey, &call));
        }
    }

    #[benchmark]
    fn check_evm_key_association_extension() {
        let netuid = NetUid::from(1);
        let hotkey: T::AccountId = account("hotkey", 0, 1);
        let block_number = T::EvmKeyAssociateRateLimit::get().saturating_add(1);
        let call = Call::<T>::associate_evm_key {
            netuid,
            evm_key: H160::zero(),
            block_number,
            signature: ecdsa::Signature::from_raw([0_u8; 65]),
        };

        setup_extension_neuron::<T>(netuid, &hotkey);
        set_benchmark_block_number::<T>(block_number);

        #[block]
        {
            assert_ok!(CheckEvmKeyAssociation::<T>::check(&hotkey, &call));
        }
    }

    #[benchmark]
    fn set_mechanism_weights(n: Linear<1, 4096>) {
        let mecid = subtensor_runtime_common::MechId::MAIN;
        let (netuid, hotkey, uids, weight_values, _salt, version_key) =
            setup_mechanism_weight_benchmark::<T>(mecid, n);
        Subtensor::<T>::set_commit_reveal_weights_enabled(netuid, false);

        #[extrinsic_call]
        _(
            RawOrigin::Signed(hotkey.clone()),
            netuid,
            mecid,
            uids,
            weight_values,
            version_key,
        );
    }

    #[benchmark]
    fn commit_mechanism_weights() {
        let mecid = subtensor_runtime_common::MechId::MAIN;
        let (netuid, hotkey, uids, weight_values, _salt, version_key) =
            setup_mechanism_weight_benchmark::<T>(mecid, 4096);
        let commit_hash: H256 =
            BlakeTwo256::hash_of(&(hotkey.clone(), netuid, uids, weight_values, version_key));
        let netuid_index = Subtensor::<T>::get_mechanism_storage_index(netuid, mecid);
        let mut commits = VecDeque::new();
        for i in 0..9u8 {
            commits.push_back((H256::repeat_byte(i + 1), 0, 0, 0));
        }
        WeightCommits::<T>::insert(netuid_index, &hotkey, commits);

        #[extrinsic_call]
        _(
            RawOrigin::Signed(hotkey.clone()),
            netuid,
            mecid,
            commit_hash,
        );
    }
    #[benchmark]
    fn reveal_mechanism_weights(n: Linear<1, 4096>) {
        let mecid = subtensor_runtime_common::MechId::MAIN;
        let netuid = NetUid::from(1);
        let netuid_index = Subtensor::<T>::get_mechanism_storage_index(netuid, mecid);
        let version_key: u64 = 0;
        let uid_count = n.clamp(1, 4096);

        // Use a non-firing benchmark subnet. This mirrors the existing
        // `reveal_weights` benchmark and removes the stateful scheduler
        // look-ahead from this extrinsic benchmark's setup. The measured code is
        // still the real reveal dispatch below; tempo is only used here to keep
        // current_epoch_with_lookahead() pinned to SubnetEpochIndex.
        Subtensor::<T>::init_new_network(netuid, 0);
        SubtokenEnabled::<T>::insert(netuid, true);
        Subtensor::<T>::set_network_registration_allowed(netuid, true);
        Subtensor::<T>::set_max_allowed_uids(netuid, 4096);
        Subtensor::<T>::set_max_registrations_per_block(netuid, 4096);
        Subtensor::<T>::set_target_registrations_per_interval(netuid, 4096);
        Subtensor::<T>::set_weights_set_rate_limit(netuid, 0);
        Subtensor::<T>::set_stake_threshold(0);
        Subtensor::<T>::set_commit_reveal_weights_enabled(netuid, true);
        Subtensor::<T>::set_burn(netuid, benchmark_registration_burn());
        set_reserves::<T>(
            netuid,
            TaoBalance::from(1_000_000_000_000_u64),
            AlphaBalance::from(1_000_000_000_000_000_u64),
        );

        let reveal_period = core::cmp::max(MIN_COMMIT_REVEAL_PEROIDS, 1_u64);
        assert_ok!(Subtensor::<T>::set_reveal_period(netuid, reveal_period));

        let mut uids = Vec::with_capacity(uid_count as usize);
        let mut weight_values = Vec::with_capacity(uid_count as usize);
        let mut signer_hotkey: Option<T::AccountId> = None;

        for seed in 0..uid_count {
            let hotkey: T::AccountId = account("mechanism_reveal_hot", seed, 1);
            let coldkey: T::AccountId = account("mechanism_reveal_cold", seed, 2);

            Burn::<T>::insert(netuid, benchmark_registration_burn());
            RegistrationsThisInterval::<T>::insert(netuid, 0);
            fund_for_registration::<T>(netuid, &coldkey);

            assert_ok!(Subtensor::<T>::burned_register(
                RawOrigin::Signed(coldkey.clone()).into(),
                netuid,
                hotkey.clone(),
            ));

            let uid = Subtensor::<T>::get_uid_for_net_and_hotkey(netuid, &hotkey).unwrap();
            Subtensor::<T>::set_validator_permit_for_uid(netuid, uid, true);

            if signer_hotkey.is_none() {
                signer_hotkey = Some(hotkey.clone());
            }
            uids.push(uid);
            weight_values.push(uid.saturating_add(1));
        }

        let hotkey = signer_hotkey.expect("at least one benchmark neuron is registered");
        let salt: Vec<u16> = vec![u16::MAX; uid_count as usize];
        let commit_hash = Subtensor::<T>::get_commit_hash(
            &hotkey,
            netuid_index,
            &uids,
            &weight_values,
            &salt,
            version_key,
        );

        // Worst-case the successful CR-v2 reveal queue. The valid commit is at
        // the back of the bounded 10-entry queue, so reveal scans and drains the
        // maximum prefix. These commits are intentionally non-expired: expired
        // front entries are a failure path for this dispatch when the provided
        // hash is among the drained hashes.
        let commit_epoch = 0_u64;
        let commit_block = Subtensor::<T>::get_current_block_as_u64();
        let mut commits = VecDeque::new();
        for i in 0..9_u8 {
            let mut dummy_hash = H256::repeat_byte(i.saturating_add(1));
            if dummy_hash == commit_hash {
                dummy_hash = H256::repeat_byte(i.saturating_add(11));
            }
            commits.push_back((dummy_hash, commit_epoch, commit_block, 0));
        }
        commits.push_back((commit_hash, commit_epoch, commit_block, 0));
        WeightCommits::<T>::insert(netuid_index, &hotkey, commits);

        // With tempo 0, should_run_epoch() is false and current_epoch_with_lookahead()
        // equals SubnetEpochIndex. Put the subnet exactly in the reveal epoch.
        LastEpochBlock::<T>::insert(netuid, 0);
        BlocksSinceLastStep::<T>::insert(netuid, 0);
        PendingEpochAt::<T>::insert(netuid, 0);
        SubnetEpochIndex::<T>::insert(netuid, reveal_period);

        assert_eq!(
            Subtensor::<T>::current_epoch_with_lookahead(netuid),
            reveal_period
        );
        assert!(Subtensor::<T>::is_reveal_block_range(netuid, commit_epoch));
        assert!(!Subtensor::<T>::is_commit_expired(netuid, commit_epoch));

        #[extrinsic_call]
        _(
            RawOrigin::Signed(hotkey.clone()),
            netuid,
            mecid,
            uids,
            weight_values,
            salt,
            version_key,
        );
    }

    #[benchmark]
    fn commit_crv3_mechanism_weights() {
        let mecid = subtensor_runtime_common::MechId::MAIN;
        let (netuid, hotkey, _uids, _weight_values, _salt, _version_key) =
            setup_mechanism_weight_benchmark::<T>(mecid, 4096);
        let vec_commit: Vec<u8> = vec![u8::MAX; MAX_CRV3_COMMIT_SIZE_BYTES as usize];
        let commit: BoundedVec<_, _> = vec_commit.try_into().unwrap();
        let netuid_index = Subtensor::<T>::get_mechanism_storage_index(netuid, mecid);
        let epoch = Subtensor::<T>::current_epoch_with_lookahead(netuid);
        let mut existing = VecDeque::new();
        for i in 0..9u64 {
            existing.push_back((hotkey.clone(), 0, commit.clone(), i));
        }
        TimelockedWeightCommits::<T>::insert(netuid_index, epoch, existing);

        #[extrinsic_call]
        _(
            RawOrigin::Signed(hotkey.clone()),
            netuid,
            mecid,
            commit,
            u64::MAX,
        );
    }

    #[benchmark]
    fn commit_timelocked_mechanism_weights() {
        let mecid = subtensor_runtime_common::MechId::MAIN;
        let (netuid, hotkey, _uids, _weight_values, _salt, _version_key) =
            setup_mechanism_weight_benchmark::<T>(mecid, 4096);
        let vec_commit: Vec<u8> = vec![u8::MAX; MAX_CRV3_COMMIT_SIZE_BYTES as usize];
        let commit: BoundedVec<_, _> = vec_commit.try_into().unwrap();
        let netuid_index = Subtensor::<T>::get_mechanism_storage_index(netuid, mecid);
        let epoch = Subtensor::<T>::current_epoch_with_lookahead(netuid);
        let mut existing = VecDeque::new();
        for i in 0..9u64 {
            existing.push_back((hotkey.clone(), 0, commit.clone(), i));
        }
        TimelockedWeightCommits::<T>::insert(netuid_index, epoch, existing);
        let version = Subtensor::<T>::get_commit_reveal_weights_version();

        #[extrinsic_call]
        _(
            RawOrigin::Signed(hotkey.clone()),
            netuid,
            mecid,
            commit,
            u64::MAX,
            version,
        );
    }

    #[benchmark]
    fn swap_hotkey_v2() {
        let coldkey: T::AccountId = whitelisted_caller();
        let old_hotkey: T::AccountId = account("old_hotkey", 0, 1);
        let new_hotkey: T::AccountId = account("new_hotkey", 0, 1);

        for netuid_raw in 1..=GLOBAL_MAX_SUBNET_COUNT {
            let netuid = NetUid::from(netuid_raw);
            Subtensor::<T>::init_new_network(netuid, 1);
            SubtokenEnabled::<T>::insert(netuid, true);
            Subtensor::<T>::set_network_registration_allowed(netuid, true);
            Burn::<T>::insert(netuid, benchmark_registration_burn());
            seed_swap_reserves::<T>(netuid);
            fund_for_registration::<T>(netuid, &coldkey);

            assert_ok!(Subtensor::<T>::burned_register(
                RawOrigin::Signed(coldkey.clone()).into(),
                netuid,
                old_hotkey.clone(),
            ));

            let alpha_amount = AlphaBalance::from(1_000_000_u64);
            SubnetAlphaOut::<T>::insert(netuid, alpha_amount * 2.into());
            Subtensor::<T>::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &old_hotkey,
                &coldkey,
                netuid,
                alpha_amount,
            );
        }

        Owner::<T>::insert(&old_hotkey, &coldkey);
        let cost = Subtensor::<T>::get_key_swap_cost();
        add_balance_to_coldkey_account::<T>(&coldkey, cost.into());

        #[extrinsic_call]
        _(
            RawOrigin::Signed(coldkey.clone()),
            old_hotkey,
            new_hotkey,
            None,
            false,
        );
    }

    #[benchmark]
    fn sudo_set_min_childkey_take() {
        #[extrinsic_call]
        _(RawOrigin::Root, PerU16::from_parts(u16::MIN));
    }

    #[benchmark]
    fn sudo_set_max_childkey_take() {
        #[extrinsic_call]
        _(RawOrigin::Root, PerU16::from_parts(u16::MAX));
    }

    #[benchmark]
    fn dissolve_network() {
        let netuid = NetUid::from(1);
        let (_hotkey, coldkey, _uids, _weights) =
            setup_worst_case_registered_subnet::<T>("dissolve", netuid, 4096);
        SubnetOwner::<T>::insert(netuid, coldkey.clone());

        #[extrinsic_call]
        _(RawOrigin::Root, coldkey.clone(), netuid);
    }

    #[benchmark]
    fn root_dissolve_network() {
        let netuid = NetUid::from(1);
        let (_hotkey, _coldkey, _uids, _weights) =
            setup_worst_case_registered_subnet::<T>("root_dissolve", netuid, 4096);

        #[extrinsic_call]
        _(RawOrigin::Root, netuid);
    }

    #[benchmark]
    fn set_children(c: Linear<1, 5>) {
        let netuid = NetUid::from(1);
        let coldkey: T::AccountId = account("children_cold", 0, 1);
        let hotkey: T::AccountId = account("children_hot", 0, 1);
        let mut children = Vec::with_capacity(c as usize);

        Subtensor::<T>::init_new_network(netuid, 1);
        Subtensor::<T>::set_network_registration_allowed(netuid, true);
        SubtokenEnabled::<T>::insert(netuid, true);
        Burn::<T>::insert(netuid, benchmark_registration_burn());
        seed_swap_reserves::<T>(netuid);
        fund_for_registration::<T>(netuid, &coldkey);

        assert_ok!(Subtensor::<T>::burned_register(
            RawOrigin::Signed(coldkey.clone()).into(),
            netuid,
            hotkey.clone(),
        ));

        for seed in 0..c {
            let child: T::AccountId = account("children_child", seed, 1);
            children.push((u64::MAX / c as u64, child));
        }

        #[extrinsic_call]
        _(RawOrigin::Signed(coldkey.clone()), hotkey, netuid, children);
    }

    #[allow(deprecated)]
    #[benchmark]
    fn schedule_swap_coldkey() {
        let new_coldkey: T::AccountId = account("new_coldkey", 0, u32::MAX);

        #[block]
        {
            assert!(
                Subtensor::<T>::schedule_swap_coldkey(RawOrigin::Root.into(), new_coldkey).is_err()
            );
        }
    }

    #[benchmark]
    fn enable_voting_power_tracking() {
        let netuid = NetUid::from(1);
        let owner: T::AccountId = whitelisted_caller();
        Subtensor::<T>::init_new_network(netuid, 1);
        SubnetOwner::<T>::insert(netuid, owner.clone());

        #[extrinsic_call]
        _(RawOrigin::Signed(owner.clone()), netuid);
    }

    #[benchmark]
    fn disable_voting_power_tracking() {
        let netuid = NetUid::from(1);
        let owner: T::AccountId = whitelisted_caller();
        Subtensor::<T>::init_new_network(netuid, 1);
        SubnetOwner::<T>::insert(netuid, owner.clone());
        assert_ok!(Subtensor::<T>::enable_voting_power_tracking(
            RawOrigin::Signed(owner.clone()).into(),
            netuid
        ));

        #[extrinsic_call]
        _(RawOrigin::Signed(owner.clone()), netuid);
    }

    #[benchmark]
    fn sudo_set_voting_power_ema_alpha() {
        let netuid = NetUid::from(1);
        Subtensor::<T>::init_new_network(netuid, 1);

        #[extrinsic_call]
        _(RawOrigin::Root, netuid, 1_000_000_000_000_000_000u64);
    }

    #[benchmark]
    fn register_limit() {
        let netuid = NetUid::from(1);
        let coldkey: T::AccountId = account("register_limit_cold", 0, 1);
        let hotkey: T::AccountId = account("register_limit_hot", 0, 1);

        setup_full_subnet_registration_benchmark::<T>(
            netuid,
            "register_limit_existing_hot",
            "register_limit_existing_cold",
        );
        // Match `burned_register`: measure the collateral-enabled payment path.
        CollateralLockShare::<T>::insert(netuid, MaxCollateralLockShare::<T>::get());
        fund_for_registration::<T>(netuid, &coldkey);

        #[extrinsic_call]
        _(RawOrigin::Signed(coldkey.clone()), netuid, hotkey, u64::MAX);
    }

    #[benchmark]
    fn set_perpetual_lock() {
        let netuid = NetUid::from(1);
        let coldkey: T::AccountId = account("perpetual_cold", 0, 1);
        let hotkey: T::AccountId = account("perpetual_hot", 0, 1);

        Subtensor::<T>::init_new_network(netuid, 1);
        Subtensor::<T>::set_network_registration_allowed(netuid, true);
        SubtokenEnabled::<T>::insert(netuid, true);
        Burn::<T>::insert(netuid, benchmark_registration_burn());
        seed_swap_reserves::<T>(netuid);
        fund_for_registration::<T>(netuid, &coldkey);

        assert_ok!(Subtensor::<T>::burned_register(
            RawOrigin::Signed(coldkey.clone()).into(),
            netuid,
            hotkey.clone(),
        ));
        add_lock::<T>(&coldkey, netuid);

        #[extrinsic_call]
        _(RawOrigin::Signed(coldkey.clone()), netuid, true);
    }

    #[benchmark]
    fn set_tempo() {
        let caller: T::AccountId = whitelisted_caller();

        #[extrinsic_call]
        _(RawOrigin::Signed(caller), NetUid::from(1), u16::MAX);
    }

    #[benchmark]
    fn set_activity_cutoff_factor() {
        let caller: T::AccountId = whitelisted_caller();

        #[extrinsic_call]
        _(RawOrigin::Signed(caller), NetUid::from(1), u32::MAX);
    }

    #[benchmark]
    fn set_reject_locked_alpha() {
        let coldkey: T::AccountId = whitelisted_caller();
        AccountFlags::<T>::insert(&coldkey, crate::ACCOUNT_FLAGS_ACCEPT_LOCKED_ALPHA);

        #[extrinsic_call]
        _(RawOrigin::Signed(coldkey.clone()), true);
    }

    impl_benchmark_test_suite!(
        Subtensor,
        crate::tests::mock::new_test_ext(1),
        crate::tests::mock::Test
    );
}
