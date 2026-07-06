use super::*;

pub(super) fn seed_swap_reserves<T: Config>(netuid: NetUid) {
    let tao_reserve = TaoBalance::from(150_000_000_000_u64);
    let alpha_in = AlphaBalance::from(100_000_000_000_u64);
    set_reserves::<T>(netuid, tao_reserve, alpha_in);
}

pub(super) fn set_reserves<T: Config>(
    netuid: NetUid,
    tao_reserve: TaoBalance,
    alpha_in: AlphaBalance,
) {
    SubnetTAO::<T>::insert(netuid, tao_reserve);
    SubnetAlphaIn::<T>::insert(netuid, alpha_in);
}

pub(super) fn benchmark_registration_burn() -> TaoBalance {
    TaoBalance::from(1_000_000)
}

pub(super) fn add_balance_to_coldkey_account<T: Config>(coldkey: &T::AccountId, tao: TaoBalance) {
    let credit = Subtensor::<T>::mint_tao(tao);
    let _ = Subtensor::<T>::spend_tao(coldkey, credit, tao).unwrap();
}

/// This helper funds an account with:
/// - 2x burn fee
/// - 100x DefaultMinStake
pub(super) fn fund_for_registration<T: Config>(netuid: NetUid, who: &T::AccountId) {
    let burn = Subtensor::<T>::get_burn(netuid);
    let min_stake = DefaultMinStake::<T>::get();

    let deposit = burn
        .saturating_mul(2.into())
        .saturating_add(min_stake.saturating_mul(100.into()));

    add_balance_to_coldkey_account::<T>(who, deposit.into());
}

pub(super) fn dense_benchmark_weights(uid_count: u16) -> Vec<(u16, u16)> {
    (0..uid_count)
        .map(|uid| (uid, u16::MAX))
        .collect::<Vec<_>>()
}

pub(super) fn seed_benchmark_neuron<T: Config>(
    netuid: NetUid,
    hotkey_label: &'static str,
    coldkey_label: &'static str,
    seed: u32,
    stake: AlphaBalance,
) -> (T::AccountId, T::AccountId) {
    let hotkey: T::AccountId = account(hotkey_label, seed, 1);
    let coldkey: T::AccountId = account(coldkey_label, seed, 2);

    assert_ok!(Subtensor::<T>::create_account_if_non_existent(
        &coldkey, &hotkey
    ));
    Subtensor::<T>::append_neuron(netuid, &hotkey, 0);
    Subtensor::<T>::increase_stake_for_hotkey_and_coldkey_on_subnet(
        &hotkey, &coldkey, netuid, stake,
    );

    (hotkey, coldkey)
}

pub(super) fn setup_full_subnet_registration_benchmark<T: Config>(
    netuid: NetUid,
    hotkey_label: &'static str,
    coldkey_label: &'static str,
) {
    let uid_count = DefaultMaxAllowedUids::<T>::get();
    assert!(
        uid_count > 0,
        "registration benchmark requires at least one subnet UID"
    );

    Subtensor::<T>::init_new_network(netuid, 1);
    SubtokenEnabled::<T>::insert(netuid, true);
    Subtensor::<T>::set_network_registration_allowed(netuid, true);
    Subtensor::<T>::set_max_allowed_uids(netuid, uid_count);
    Subtensor::<T>::set_max_registrations_per_block(netuid, uid_count);
    Subtensor::<T>::set_target_registrations_per_interval(netuid, uid_count);
    Subtensor::<T>::set_burn(netuid, benchmark_registration_burn());
    seed_swap_reserves::<T>(netuid);

    let netuid_index = NetUidStorageIndex::from(netuid);
    let dense_weights = dense_benchmark_weights(uid_count);

    for uid in 0..uid_count {
        seed_benchmark_neuron::<T>(
            netuid,
            hotkey_label,
            coldkey_label,
            u32::from(uid),
            AlphaBalance::ZERO,
        );
        Subtensor::<T>::set_validator_permit_for_uid(netuid, uid, true);
        Weights::<T>::insert(netuid_index, uid, dense_weights.clone());
        Bonds::<T>::insert(netuid_index, uid, dense_weights.clone());
    }

    assert_eq!(Subtensor::<T>::get_subnetwork_n(netuid), uid_count);
}

pub(super) fn setup_full_root_registration_benchmark<T: Config>() {
    Subtensor::<T>::init_new_network(NetUid::ROOT, 1);
    SubtokenEnabled::<T>::insert(NetUid::ROOT, true);
    Subtensor::<T>::set_network_registration_allowed(NetUid::ROOT, true);
    FirstEmissionBlockNumber::<T>::insert(NetUid::ROOT, 1);

    let root_validator_count = Subtensor::<T>::get_max_allowed_validators(NetUid::ROOT);
    assert!(
        root_validator_count > 0,
        "root registration benchmark requires at least one root validator"
    );

    Subtensor::<T>::set_max_allowed_uids(NetUid::ROOT, root_validator_count);
    Subtensor::<T>::set_max_registrations_per_block(
        NetUid::ROOT,
        root_validator_count.saturating_add(1),
    );
    Subtensor::<T>::set_target_registrations_per_interval(
        NetUid::ROOT,
        root_validator_count.saturating_add(1),
    );

    let netuid_index = NetUidStorageIndex::from(NetUid::ROOT);
    let dense_weights = dense_benchmark_weights(root_validator_count);

    for uid in 0..root_validator_count {
        seed_benchmark_neuron::<T>(
            NetUid::ROOT,
            "root_register_existing_hot",
            "root_register_existing_cold",
            u32::from(uid),
            AlphaBalance::from(1_u64),
        );
        Subtensor::<T>::set_validator_permit_for_uid(NetUid::ROOT, uid, true);
        Weights::<T>::insert(netuid_index, uid, dense_weights.clone());
        Bonds::<T>::insert(netuid_index, uid, dense_weights.clone());
    }

    assert_eq!(
        Subtensor::<T>::get_subnetwork_n(NetUid::ROOT),
        root_validator_count
    );
}

/// Add a zero lock to a random hotkey just so that the lock records exist
pub(super) fn add_lock<T: Config>(coldkey: &T::AccountId, netuid: NetUid) {
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

pub(super) fn set_benchmark_block_number<T: Config>(block_number: u64) {
    let block_number: BlockNumberFor<T> = match block_number.try_into() {
        Ok(block_number) => block_number,
        Err(_) => panic!("benchmark block number must fit into BlockNumberFor<T>"),
    };

    frame_system::Pallet::<T>::set_block_number(block_number);
}

/// Seed a mainnet-like block_step state without measuring registration costs.
///
/// The measured block_step path iterates over subnets repeatedly: burn-price
/// updates, coinbase, moving prices, root proportion, pending children,
/// root dividend auto-claims, and root coldkey map population. Keep the setup
/// intentionally large so this hook benchmark reflects a 128-subnet mainnet
/// state instead of a one-subnet toy state.
///
/// Only the runtime-capped number of subnet epochs are made due in the measured
/// block. The remaining subnets are fully populated and live, but not eligible
/// for epoch execution in this block. This mirrors production behavior where
/// `MaxEpochsPerBlock` bounds the number of Yuma epochs that can execute in a
/// single block.
pub(super) fn setup_block_step_benchmark<T: Config>() {
    const MAINNET_SUBNETS: u16 = 128;
    const MAINNET_NEURONS_PER_SUBNET: u16 = 256;
    const MAINNET_VALIDATORS_PER_SUBNET: u16 = 128;
    const TEMPO: u16 = 360;
    const CURRENT_BLOCK: u64 = 7_000;
    const SUBNET_TAO_RESERVE: u64 = 1_000_000_000_000_000;
    const SUBNET_ALPHA_RESERVE: u64 = 1_000_000_000_000_000_000;
    const VALIDATOR_ALPHA_STAKE: u64 = 1_000_000_000;

    set_benchmark_block_number::<T>(CURRENT_BLOCK);

    let max_epochs_this_block =
        u16::from(Subtensor::<T>::get_max_epochs_per_block()).min(MAINNET_SUBNETS);
    assert!(
        max_epochs_this_block > 0,
        "block_step benchmark requires MaxEpochsPerBlock > 0"
    );

    let dense_weights = (0..MAINNET_NEURONS_PER_SUBNET)
        .map(|dest_uid| (dest_uid, u16::MAX))
        .collect::<Vec<_>>();

    Subtensor::<T>::init_new_network(NetUid::ROOT, TEMPO);
    Subtensor::<T>::set_network_registration_allowed(NetUid::ROOT, true);
    Subtensor::<T>::set_max_allowed_uids(NetUid::ROOT, MAINNET_NEURONS_PER_SUBNET);
    FirstEmissionBlockNumber::<T>::insert(NetUid::ROOT, CURRENT_BLOCK);
    LastEpochBlock::<T>::insert(NetUid::ROOT, CURRENT_BLOCK);
    LastMechansimStepBlock::<T>::insert(NetUid::ROOT, CURRENT_BLOCK);
    BlocksSinceLastStep::<T>::insert(NetUid::ROOT, 0);
    PendingEpochAt::<T>::insert(NetUid::ROOT, 0);
    SubtokenEnabled::<T>::insert(NetUid::ROOT, true);
    SubnetEmissionEnabled::<T>::insert(NetUid::ROOT, true);
    set_reserves::<T>(
        NetUid::ROOT,
        TaoBalance::from(SUBNET_TAO_RESERVE),
        AlphaBalance::from(SUBNET_ALPHA_RESERVE),
    );
    SubnetAlphaOut::<T>::insert(NetUid::ROOT, AlphaBalance::from(SUBNET_ALPHA_RESERVE));

    // Root is also populated because block_step ends by rebuilding root coldkey
    // staking maps. These root neurons mirror the high-cardinality account map
    // shape seen on a populated mainnet chain.
    let root_index = NetUidStorageIndex::from(NetUid::ROOT);
    for uid in 0..MAINNET_NEURONS_PER_SUBNET {
        let hotkey: T::AccountId = account("block_step_root_hot", 0, u32::from(uid));
        let coldkey: T::AccountId = account("block_step_root_cold", 0, u32::from(uid));

        Owner::<T>::insert(&hotkey, &coldkey);
        Subtensor::<T>::append_neuron(NetUid::ROOT, &hotkey, 0);
        Subtensor::<T>::set_validator_permit_for_uid(NetUid::ROOT, uid, true);
        Subtensor::<T>::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            NetUid::ROOT,
            AlphaBalance::from(VALIDATOR_ALPHA_STAKE),
        );

        if uid < MAINNET_VALIDATORS_PER_SUBNET {
            Weights::<T>::insert(root_index, uid, dense_weights.clone());
            Bonds::<T>::insert(root_index, uid, dense_weights.clone());
        }
    }

    // 128 live non-root subnets, each with 256 registered neurons. The first 128
    // neurons per subnet are validator-permit neurons with dense weight and bond
    // rows. Only the first `MaxEpochsPerBlock` subnets are scheduled to run their
    // epoch in this measured block; the rest remain live ambient state.
    for subnet_index in 1..=MAINNET_SUBNETS {
        let netuid = NetUid::from(subnet_index);
        let netuid_index = NetUidStorageIndex::from(netuid);
        let subnet_owner: T::AccountId =
            account("block_step_subnet_owner", u32::from(subnet_index), 0);
        let epoch_is_due_this_block = subnet_index <= max_epochs_this_block;

        Subtensor::<T>::init_new_network(netuid, TEMPO);
        SubtokenEnabled::<T>::insert(netuid, true);
        SubnetEmissionEnabled::<T>::insert(netuid, true);
        SubnetOwner::<T>::insert(netuid, subnet_owner);
        Subtensor::<T>::set_network_registration_allowed(netuid, true);
        Subtensor::<T>::set_max_allowed_uids(netuid, MAINNET_NEURONS_PER_SUBNET);
        Subtensor::<T>::set_max_registrations_per_block(netuid, MAINNET_NEURONS_PER_SUBNET);
        Subtensor::<T>::set_target_registrations_per_interval(netuid, MAINNET_NEURONS_PER_SUBNET);
        Subtensor::<T>::set_burn(netuid, benchmark_registration_burn());
        set_reserves::<T>(
            netuid,
            TaoBalance::from(SUBNET_TAO_RESERVE),
            AlphaBalance::from(SUBNET_ALPHA_RESERVE),
        );
        SubnetAlphaOut::<T>::insert(netuid, AlphaBalance::from(SUBNET_ALPHA_RESERVE));
        FirstEmissionBlockNumber::<T>::insert(netuid, CURRENT_BLOCK);

        if epoch_is_due_this_block {
            PendingEpochAt::<T>::insert(netuid, CURRENT_BLOCK);
            LastEpochBlock::<T>::insert(netuid, 0);
            LastMechansimStepBlock::<T>::insert(netuid, 0);
            BlocksSinceLastStep::<T>::insert(netuid, CURRENT_BLOCK);
        } else {
            PendingEpochAt::<T>::insert(netuid, 0);
            LastEpochBlock::<T>::insert(netuid, CURRENT_BLOCK);
            LastMechansimStepBlock::<T>::insert(netuid, CURRENT_BLOCK);
            BlocksSinceLastStep::<T>::insert(netuid, 0);
        }

        for uid in 0..MAINNET_NEURONS_PER_SUBNET {
            let hotkey: T::AccountId =
                account("block_step_hot", u32::from(subnet_index), u32::from(uid));
            let coldkey: T::AccountId =
                account("block_step_cold", u32::from(subnet_index), u32::from(uid));

            Owner::<T>::insert(&hotkey, &coldkey);
            Subtensor::<T>::append_neuron(netuid, &hotkey, 0);
            Subtensor::<T>::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &coldkey,
                netuid,
                AlphaBalance::from(VALIDATOR_ALPHA_STAKE),
            );

            if uid < MAINNET_VALIDATORS_PER_SUBNET {
                Subtensor::<T>::set_validator_permit_for_uid(netuid, uid, true);
                Weights::<T>::insert(netuid_index, uid, dense_weights.clone());
                Bonds::<T>::insert(netuid_index, uid, dense_weights.clone());
            }
        }
    }
}

pub(super) fn runtime_call<T: Config>(call: Call<T>) -> <T as frame_system::Config>::RuntimeCall {
    <T as Config>::RuntimeCall::from(call).into()
}

pub(super) fn setup_extension_neuron<T: Config>(netuid: NetUid, hotkey: &T::AccountId) {
    Subtensor::<T>::init_new_network(netuid, 0);
    Subtensor::<T>::set_max_allowed_uids(netuid, GLOBAL_MAX_SUBNET_COUNT);
    Subtensor::<T>::append_neuron(netuid, hotkey, 0);
}

pub(super) fn benchmark_evm_secret_key() -> libsecp256k1::SecretKey {
    let seed = [42u8; 32];

    match libsecp256k1::SecretKey::parse(&seed) {
        Ok(secret_key) => secret_key,
        Err(_) => panic!("benchmark EVM secret key must be valid"),
    }
}

pub(super) fn evm_key_from_secret_key(secret_key: &libsecp256k1::SecretKey) -> H160 {
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

pub(super) fn signature_for_associate_evm_key<T: Config>(
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

pub(super) fn setup_worst_case_registered_subnet<T: Config>(
    label: &'static str,
    netuid: NetUid,
    uid_count: u32,
) -> (T::AccountId, T::AccountId, Vec<u16>, Vec<u16>) {
    let uid_count = uid_count.clamp(1, 4096);
    let mut uids = Vec::with_capacity(uid_count as usize);
    let mut weights = Vec::with_capacity(uid_count as usize);
    let mut signer_hotkey: Option<T::AccountId> = None;
    let mut signer_coldkey: Option<T::AccountId> = None;

    Subtensor::<T>::init_new_network(netuid, 1);
    SubtokenEnabled::<T>::insert(netuid, true);
    Subtensor::<T>::set_network_registration_allowed(netuid, true);
    Subtensor::<T>::set_max_allowed_uids(netuid, 4096);
    Subtensor::<T>::set_max_registrations_per_block(netuid, 4096);
    Subtensor::<T>::set_target_registrations_per_interval(netuid, 4096);
    Subtensor::<T>::set_difficulty(netuid, 1);
    Subtensor::<T>::set_weights_set_rate_limit(netuid, 0);
    Subtensor::<T>::set_stake_threshold(0);
    set_reserves::<T>(
        netuid,
        TaoBalance::from(1_000_000_000_000_u64),
        AlphaBalance::from(1_000_000_000_000_000_u64),
    );

    for seed in 0..uid_count {
        let hotkey: T::AccountId = account(label, seed, 1);
        let coldkey: T::AccountId = account(label, seed, 2);
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
        uids.push(uid);
        weights.push(uid.saturating_add(1));

        if signer_hotkey.is_none() {
            signer_hotkey = Some(hotkey);
            signer_coldkey = Some(coldkey);
        }
    }

    (
        signer_hotkey.unwrap(),
        signer_coldkey.unwrap(),
        uids,
        weights,
    )
}

pub(super) fn setup_mechanism_weight_benchmark<T: Config>(
    _mecid: subtensor_runtime_common::MechId,
    uid_count: u32,
) -> (NetUid, T::AccountId, Vec<u16>, Vec<u16>, Vec<u16>, u64) {
    let netuid = NetUid::from(1);
    let version_key: u64 = 0;
    let (hotkey, _coldkey, uids, weight_values) =
        setup_worst_case_registered_subnet::<T>("mechanism", netuid, uid_count);
    let salt: Vec<u16> = vec![u16::MAX; uid_count.clamp(1, 4096) as usize];
    Subtensor::<T>::set_commit_reveal_weights_enabled(netuid, true);

    (netuid, hotkey, uids, weight_values, salt, version_key)
}
