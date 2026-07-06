//! Subtensor pallet benchmarking.
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used)]
#![cfg(feature = "runtime-benchmarks")]

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
    BoundedVec, Percent,
    traits::{BlakeTwo256, Dispatchable, Hash},
};
use sp_std::collections::{btree_set::BTreeSet, vec_deque::VecDeque};
use sp_std::vec;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{AlphaBalance, NetUid, NetUidStorageIndex, TaoBalance};
use subtensor_swap_interface::SwapHandler;

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
    use super::*;

    fn seed_swap_reserves<T: Config>(netuid: NetUid) {
        let tao_reserve = TaoBalance::from(150_000_000_000_u64);
        let alpha_in = AlphaBalance::from(100_000_000_000_u64);
        set_reserves::<T>(netuid, tao_reserve, alpha_in);
    }

    fn set_reserves<T: Config>(netuid: NetUid, tao_reserve: TaoBalance, alpha_in: AlphaBalance) {
        SubnetTAO::<T>::insert(netuid, tao_reserve);
        SubnetAlphaIn::<T>::insert(netuid, alpha_in);
    }

    fn benchmark_registration_burn() -> TaoBalance {
        TaoBalance::from(1_000_000)
    }

    fn add_balance_to_coldkey_account<T: Config>(coldkey: &T::AccountId, tao: TaoBalance) {
        let credit = Subtensor::<T>::mint_tao(tao);
        let _ = Subtensor::<T>::spend_tao(coldkey, credit, tao).unwrap();
    }

    /// This helper funds an account with:
    /// - 2x burn fee
    /// - 100x DefaultMinStake
    fn fund_for_registration<T: Config>(netuid: NetUid, who: &T::AccountId) {
        let burn = Subtensor::<T>::get_burn(netuid);
        let min_stake = DefaultMinStake::<T>::get();

        let deposit = burn
            .saturating_mul(2.into())
            .saturating_add(min_stake.saturating_mul(100.into()));

        add_balance_to_coldkey_account::<T>(who, deposit.into());
    }

    /// Add a zero lock to a random hotkey just so that the lock records exist
    fn add_lock<T: Config>(coldkey: &T::AccountId, netuid: NetUid) {
        let hotkey: T::AccountId = account("RandomHotkey", 0, 999);
        Lock::<T>::insert(
            (coldkey, netuid, hotkey.clone()),
            LockState {
                locked_mass: AlphaBalance::ZERO,
                conviction: U64F64::from_num(0),
                last_update: 0,
            },
        );
        HotkeyLock::<T>::insert(
            netuid,
            hotkey,
            LockState {
                locked_mass: AlphaBalance::ZERO,
                conviction: U64F64::from_num(0),
                last_update: 0,
            },
        );
    }

    fn set_benchmark_block_number<T: Config>(block_number: u64) {
        let block_number: BlockNumberFor<T> = match block_number.try_into() {
            Ok(block_number) => block_number,
            Err(_) => panic!("benchmark block number must fit into BlockNumberFor<T>"),
        };

        frame_system::Pallet::<T>::set_block_number(block_number);
    }

    fn runtime_call<T: Config>(call: Call<T>) -> <T as frame_system::Config>::RuntimeCall {
        <T as Config>::RuntimeCall::from(call).into()
    }

    fn setup_extension_neuron<T: Config>(netuid: NetUid, hotkey: &T::AccountId) {
        Subtensor::<T>::init_new_network(netuid, 0);
        Subtensor::<T>::set_max_allowed_uids(netuid, GLOBAL_MAX_SUBNET_COUNT);
        Subtensor::<T>::append_neuron(netuid, hotkey, 0);
    }

    fn benchmark_evm_secret_key() -> libsecp256k1::SecretKey {
        let seed = [42u8; 32];

        match libsecp256k1::SecretKey::parse(&seed) {
            Ok(secret_key) => secret_key,
            Err(_) => panic!("benchmark EVM secret key must be valid"),
        }
    }

    fn evm_key_from_secret_key(secret_key: &libsecp256k1::SecretKey) -> H160 {
        let public_key = libsecp256k1::PublicKey::from_secret_key(secret_key);
        let uncompressed = public_key.serialize();

        let public_key_without_prefix = match uncompressed.get(1..) {
            Some(public_key_without_prefix) => public_key_without_prefix,
            None => panic!("uncompressed secp256k1 public key must contain a prefix byte"),
        };

        let hashed_public_key = sp_io::hashing::keccak_256(public_key_without_prefix);

        let evm_key_bytes = match hashed_public_key.get(12..) {
            Some(evm_key_bytes) => evm_key_bytes,
            None => panic!("keccak256 hash must be 32 bytes"),
        };

        H160::from_slice(evm_key_bytes)
    }

    fn signature_for_associate_evm_key<T: Config>(
        hotkey: &T::AccountId,
        block_number: u64,
        secret_key: &libsecp256k1::SecretKey,
    ) -> ecdsa::Signature {
        let block_hash = sp_io::hashing::keccak_256(block_number.encode().as_ref());

        let mut message = hotkey.encode();
        message.extend_from_slice(&block_hash);

        let message_hash = Subtensor::<T>::hash_message_eip191(message);
        let secp_message = libsecp256k1::Message::parse(&message_hash);

        let (secp_signature, recovery_id) = libsecp256k1::sign(&secp_message, secret_key);

        let mut signature = [0u8; 65];
        let serialized_signature = secp_signature.serialize();

        let signature_bytes = match signature.get_mut(..64) {
            Some(signature_bytes) => signature_bytes,
            None => panic!("benchmark ECDSA signature buffer must contain 64 signature bytes"),
        };
        signature_bytes.copy_from_slice(&serialized_signature);

        let recovery_id_byte = match signature.get_mut(64) {
            Some(recovery_id_byte) => recovery_id_byte,
            None => panic!("benchmark ECDSA signature buffer must contain a recovery id byte"),
        };
        *recovery_id_byte = recovery_id.serialize();

        ecdsa::Signature::from_raw(signature)
    }

    #[benchmark]
    fn register() {
        let netuid = NetUid::from(1);
        let tempo: u16 = 1;
        let hotkey: T::AccountId = account("Alice", 0, 1);
        let coldkey: T::AccountId = account("Test", 0, 2);

        Subtensor::<T>::init_new_network(netuid, tempo);
        Subtensor::<T>::set_max_allowed_uids(netuid, 4096);
        SubtokenEnabled::<T>::insert(netuid, true);

        Subtensor::<T>::set_network_registration_allowed(netuid, true);

        Subtensor::<T>::set_burn(netuid, benchmark_registration_burn());
        seed_swap_reserves::<T>(netuid);

        fund_for_registration::<T>(netuid, &coldkey);
        fund_for_registration::<T>(netuid, &hotkey);
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
        let seed: u32 = 1;
        let hotkey: T::AccountId = account("Alice", 0, seed);
        let coldkey: T::AccountId = account("Test", 0, seed);

        Subtensor::<T>::init_new_network(netuid, 1);
        SubtokenEnabled::<T>::insert(netuid, true);
        Subtensor::<T>::set_burn(netuid, benchmark_registration_burn());

        let amount: u64 = 1_000_000;
        add_balance_to_coldkey_account::<T>(&coldkey, amount.into());

        #[extrinsic_call]
        _(RawOrigin::Signed(coldkey.clone()), netuid, hotkey.clone());
    }

    #[benchmark]
    fn root_register() {
        let netuid = NetUid::from(1);
        let seed: u32 = 1;
        let coldkey: T::AccountId = account("Test", 0, seed);
        let hotkey: T::AccountId = account("Alice", 0, seed);

        Subtensor::<T>::init_new_network(netuid, 1);
        SubtokenEnabled::<T>::insert(netuid, true);
        Subtensor::<T>::set_burn(netuid, benchmark_registration_burn());
        Subtensor::<T>::set_network_registration_allowed(netuid, true);
        Subtensor::<T>::set_max_allowed_uids(netuid, 4096);
        assert_eq!(Subtensor::<T>::get_max_allowed_uids(netuid), 4096);
        Subtensor::<T>::init_new_network(NetUid::ROOT, 1);
        Subtensor::<T>::set_network_registration_allowed(NetUid::ROOT, true);
        FirstEmissionBlockNumber::<T>::insert(NetUid::ROOT, 1);
        SubtokenEnabled::<T>::insert(NetUid::ROOT, true);

        let amount: u64 = 100_000_000_000_000;
        seed_swap_reserves::<T>(netuid);
        add_balance_to_coldkey_account::<T>(&coldkey, amount.into());

        assert_ok!(Subtensor::<T>::burned_register(
            RawOrigin::Signed(coldkey.clone()).into(),
            netuid,
            hotkey.clone()
        ));

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
        let take: u16 = 1000;

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

        let wallet_bal = 1000000u32.into();
        add_balance_to_coldkey_account::<T>(&coldkey.clone(), wallet_bal);

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

        let amount_unstaked = AlphaBalance::from(30_000_000_000_u64);

        #[extrinsic_call]
        _(
            RawOrigin::Signed(coldkey.clone()),
            hotkey.clone(),
            netuid,
            amount_unstaked,
        );
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

        let wallet_bal = 1000000u32.into();
        add_balance_to_coldkey_account::<T>(&coldkey.clone(), wallet_bal);
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

        let amount_unstaked = AlphaBalance::from(30_000_000_000_u64);

        let current_price = T::SwapInterface::current_alpha_price(netuid);
        let limit = current_price
            .saturating_mul(U64F64::saturating_from_num(999_900_000))
            .saturating_to_num::<u64>()
            .into();

        #[extrinsic_call]
        _(
            RawOrigin::Signed(coldkey.clone()),
            hotkey.clone(),
            netuid,
            amount_unstaked,
            limit,
            true,
        );
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
        let limit_swap = TaoBalance::from(1_000_000_000_u64);
        let amount_to_be_staked = TaoBalance::from(440_000_000_000_u64);
        let amount_swapped = AlphaBalance::from(30_000_000_000_u64);
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
        let take: u16 = min_take;
        let current_take = min_take.saturating_add(1);

        Delegates::<T>::insert(&hotkey, current_take);
        Owner::<T>::insert(&hotkey, &coldkey);

        #[extrinsic_call]
        _(RawOrigin::Signed(coldkey.clone()), hotkey.clone(), take);
    }

    #[benchmark]
    fn increase_take() {
        let coldkey: T::AccountId = whitelisted_caller();
        let hotkey: T::AccountId = account("Alice", 0, 2);
        let take: u16 = 150;

        Delegates::<T>::insert(&hotkey, 100u16);
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

        let num_subnets: u16 = 4;
        for i in 1..=num_subnets {
            let netuid = NetUid::from(i);
            Subtensor::<T>::init_new_network(netuid, 1);
            SubtokenEnabled::<T>::insert(netuid, true);
            Subtensor::<T>::set_network_registration_allowed(netuid, true);
            Subtensor::<T>::set_burn(netuid, benchmark_registration_burn());

            let reg_balance = TaoBalance::from(1_000_000_u64);
            seed_swap_reserves::<T>(netuid);
            add_balance_to_coldkey_account::<T>(&coldkey, reg_balance.into());

            assert_ok!(Subtensor::<T>::burned_register(
                RawOrigin::Signed(coldkey.clone()).into(),
                netuid,
                old.clone()
            ));

            let alpha_amount = AlphaBalance::from(1_000_000_u64);
            SubnetAlphaOut::<T>::insert(netuid, alpha_amount * 2.into());
            Subtensor::<T>::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &old,
                &coldkey,
                netuid,
                alpha_amount,
            );
        }

        Owner::<T>::insert(&old, &coldkey);
        let cost = Subtensor::<T>::get_key_swap_cost();
        add_balance_to_coldkey_account::<T>(&coldkey, cost.into());

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

        add_balance_to_coldkey_account::<T>(&coldkey.clone(), 1000000u32.into());

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

        let wallet_bal = 1000000u32.into();
        add_balance_to_coldkey_account::<T>(&coldkey.clone(), wallet_bal);
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
    fn set_root_claim_type() {
        let coldkey: T::AccountId = whitelisted_caller();

        #[extrinsic_call]
        _(RawOrigin::Signed(coldkey.clone()), RootClaimTypeEnum::Keep);
    }

    #[benchmark]
    fn claim_root() {
        let coldkey: T::AccountId = whitelisted_caller();
        let hotkey: T::AccountId = account("A", 0, 1);

        let netuid = Subtensor::<T>::get_next_netuid();

        let lock_cost = Subtensor::<T>::get_network_lock_cost();
        add_balance_to_coldkey_account::<T>(&coldkey, lock_cost.into());

        assert_ok!(Subtensor::<T>::register_network(
            RawOrigin::Signed(coldkey.clone()).into(),
            hotkey.clone()
        ));

        SubtokenEnabled::<T>::insert(netuid, true);

        Subtensor::<T>::set_network_registration_allowed(netuid, true);

        NetworkRegistrationAllowed::<T>::insert(netuid, true);
        FirstEmissionBlockNumber::<T>::insert(netuid, 0);

        SubnetMechanism::<T>::insert(netuid, 1);
        SubnetworkN::<T>::insert(netuid, 1);
        Subtensor::<T>::set_tao_weight(u64::MAX);

        let root_stake = 100_000_000u64;
        Subtensor::<T>::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            NetUid::ROOT,
            root_stake.into(),
        );

        let initial_total_hotkey_alpha = 100_000_000u64;
        Subtensor::<T>::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid,
            initial_total_hotkey_alpha.into(),
        );

        let pending_root_alpha = 10_000_000u64;
        Subtensor::<T>::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            pending_root_alpha.into(),
            pending_root_alpha.into(),
            AlphaBalance::ZERO,
        );

        let initial_stake =
            Subtensor::<T>::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);

        assert_ok!(Subtensor::<T>::set_root_claim_type(
            RawOrigin::Signed(coldkey.clone()).into(),
            RootClaimTypeEnum::Keep
        ));

        #[extrinsic_call]
        _(RawOrigin::Signed(coldkey.clone()), BTreeSet::from([netuid]));

        let new_stake =
            Subtensor::<T>::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);

        assert!(new_stake > initial_stake);
    }

    #[benchmark]
    fn sudo_set_num_root_claims() {
        #[extrinsic_call]
        _(RawOrigin::Root, 40);
    }

    #[benchmark]
    fn sudo_set_root_claim_threshold() {
        let coldkey: T::AccountId = whitelisted_caller();
        let hotkey: T::AccountId = account("A", 0, 1);

        let netuid = Subtensor::<T>::get_next_netuid();

        let lock_cost = Subtensor::<T>::get_network_lock_cost();
        add_balance_to_coldkey_account::<T>(&coldkey, lock_cost.into());

        assert_ok!(Subtensor::<T>::register_network(
            RawOrigin::Signed(coldkey.clone()).into(),
            hotkey.clone()
        ));

        #[extrinsic_call]
        _(RawOrigin::Root, netuid, 100);
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
    }

    #[benchmark]
    fn set_tempo() {
        let netuid = NetUid::from(1);
        let coldkey: T::AccountId = account("Owner", 0, 1);

        Subtensor::<T>::init_new_network(netuid, 1u16);
        SubnetOwner::<T>::insert(netuid, coldkey.clone());
        SubtokenEnabled::<T>::insert(netuid, true);
        Subtensor::<T>::set_commit_reveal_weights_enabled(netuid, false);
        Subtensor::<T>::set_admin_freeze_window(0);

        #[extrinsic_call]
        _(RawOrigin::Signed(coldkey.clone()), netuid, MIN_TEMPO);
    }

    #[benchmark]
    fn set_activity_cutoff_factor() {
        let netuid = NetUid::from(1);
        let coldkey: T::AccountId = account("Owner", 0, 1);

        Subtensor::<T>::init_new_network(netuid, 1u16);
        SubnetOwner::<T>::insert(netuid, coldkey.clone());
        SubtokenEnabled::<T>::insert(netuid, true);
        Subtensor::<T>::set_admin_freeze_window(0);

        #[extrinsic_call]
        _(
            RawOrigin::Signed(coldkey.clone()),
            netuid,
            INITIAL_ACTIVITY_CUTOFF_FACTOR_MILLI,
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
            take: Subtensor::<T>::get_max_delegate_take(),
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

    impl_benchmark_test_suite!(
        Subtensor,
        crate::tests::mock::new_test_ext(1),
        crate::tests::mock::Test
    );
}
