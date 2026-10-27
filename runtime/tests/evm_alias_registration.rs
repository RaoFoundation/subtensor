//! Production-runtime regressions for assets on a legacy EVM backing account.
#![allow(clippy::unwrap_used)]

use frame_support::assert_ok;
use node_subtensor_runtime::{
    Balances, BuildStorage, EVM, HashedAccounts, Runtime, RuntimeGenesisConfig, RuntimeOrigin,
    SubtensorModule, System, evm_origin,
};
use pallet_evm::{AddressMapping, FeeCalculator};
use pallet_hashed_accounts::{DerivedDescriptor, RegistrationDescriptor};
use sp_core::{H160, U256};
use sp_runtime::{AccountId32, DispatchError, Saturating, traits::BlakeTwo256};
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance};

type Mapping = <Runtime as pallet_evm::Config>::AddressMapping;

fn ext() -> sp_io::TestExternalities {
    let mut ext: sp_io::TestExternalities = RuntimeGenesisConfig::default()
        .build_storage()
        .unwrap()
        .into();
    ext.execute_with(|| {
        System::set_block_number(1);
        pallet_admin_utils::HashedAccountsEnabled::<Runtime>::put(true);
    });
    ext
}

fn fund(account: &AccountId32) {
    assert_ok!(Balances::force_set_balance(
        RuntimeOrigin::root(),
        account.clone().into(),
        TaoBalance::new(1_000_000_000_000),
    ));
}

fn derived_account() -> (AccountId32, DerivedDescriptor, H160, AccountId32) {
    let sponsor = AccountId32::new([41; 32]);
    fund(&sponsor);
    let descriptor = subtensor_hashed::mldsa::Descriptor {
        version: 2,
        algorithm: 4,
        initial_commitment: [42; 64],
    };
    let parent = AccountId32::new(subtensor_hashed::mldsa::account_id(&descriptor));
    assert_ok!(HashedAccounts::register(
        RuntimeOrigin::signed(sponsor),
        RegistrationDescriptor::MlDsa65(descriptor),
    ));
    fund(&parent);
    let descriptor = DerivedDescriptor::Derivative {
        parent: parent.clone(),
        index: 7,
    };
    let account = descriptor.account_id();
    let alias = H160::from_slice(&AsRef::<[u8; 32]>::as_ref(&account)[..20]);
    let backing = pallet_evm::HashedAddressMapping::<BlakeTwo256>::into_account_id(alias);
    (parent, descriptor, alias, backing)
}

fn transfer_incoming_stake(backing: &AccountId32) -> (AccountId32, NetUid, AlphaBalance) {
    let donor = AccountId32::new([43; 32]);
    let hotkey = AccountId32::new([44; 32]);
    let netuid = NetUid::from(1);
    let amount = AlphaBalance::new(10_000_000_000);
    fund(&donor);
    SubtensorModule::init_new_network(netuid, 0);
    pallet_subtensor::SubnetMechanism::<Runtime>::insert(netuid, 0u16);
    pallet_subtensor::SubtokenEnabled::<Runtime>::insert(netuid, true);
    fund(&SubtensorModule::get_subnet_account_id(netuid).unwrap());
    let _ = SubtensorModule::create_account_if_non_existent(&donor, &hotkey);
    SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
        &hotkey, &donor, netuid, amount,
    );
    assert_ok!(SubtensorModule::transfer_stake(
        RuntimeOrigin::signed(donor),
        backing.clone(),
        hotkey.clone(),
        netuid,
        netuid,
        amount,
    ));
    assert_eq!(
        SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, backing, netuid),
        amount,
    );
    (hotkey, netuid, amount)
}

#[test]
fn alias_registration_rejects_incoming_stake_without_system_references() {
    ext().execute_with(|| {
        let (parent, descriptor, alias, backing) = derived_account();
        let (hotkey, netuid, amount) = transfer_incoming_stake(&backing);
        let state = System::account(&backing);
        assert_eq!(state.nonce, 0);
        assert_eq!(
            (state.providers, state.consumers, state.sufficients),
            (0, 0, 0)
        );
        let before = sp_io::storage::root(sp_runtime::StateVersion::V1);

        let owner = descriptor.account_id();
        assert_eq!(
            HashedAccounts::register_derived(RuntimeOrigin::signed(parent), descriptor),
            Err(DispatchError::Other("HashedEvmAliasAlreadyUsed")),
        );

        assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), before);
        assert_eq!(Mapping::into_account_id(alias), backing);
        assert!(evm_origin::hashed_owner(&alias).is_none());
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &backing, netuid),
            amount,
        );

        // The original native-to-EVM route still reaches the stake. Give the
        // backing account gas money and execute the real staking precompile.
        fund(&backing);
        pallet_evm::DisableWhitelistCheck::<Runtime>::put(true);
        let mut input =
            sp_io::hashing::keccak_256(b"removeStake(bytes32,uint256,uint256)")[..4].to_vec();
        input.extend(AsRef::<[u8; 32]>::as_ref(&hotkey));
        input.extend(U256::from(u64::from(amount)).to_big_endian());
        input.extend(U256::from(u16::from(netuid)).to_big_endian());
        let balance_before = Balances::free_balance(&backing);
        assert_ok!(EVM::call(
            RuntimeOrigin::signed(owner),
            alias,
            H160::from_low_u64_be(2053),
            input,
            U256::zero(),
            1_000_000,
            <Runtime as pallet_evm::Config>::FeeCalculator::min_gas_price().0,
            None,
            None,
            Vec::new(),
            Vec::new(),
        ));
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &backing, netuid),
            AlphaBalance::new(0),
        );
        assert!(Balances::free_balance(&backing) > balance_before);
    });
}

#[test]
fn alias_registration_rejects_stake_without_sweeping_prefunded_tao_or_reserving_deposit() {
    ext().execute_with(|| {
        let (parent, descriptor, alias, backing) = derived_account();
        transfer_incoming_stake(&backing);
        fund(&backing);
        let state = System::account(&backing);
        assert_eq!(state.nonce, 0);
        assert_eq!(
            (state.providers, state.consumers, state.sufficients),
            (1, 0, 0)
        );
        let before = sp_io::storage::root(sp_runtime::StateVersion::V1);
        assert_eq!(
            HashedAccounts::register_derived(RuntimeOrigin::signed(parent), descriptor),
            Err(DispatchError::Other("HashedEvmAliasAlreadyUsed")),
        );
        assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), before);
        assert_eq!(Mapping::into_account_id(alias), backing);
    });
}

#[test]
fn alias_registration_rejects_other_staking_roles_without_system_references() {
    for role in ["coldkey", "hotkey", "claim-only"] {
        ext().execute_with(|| {
            let (parent, descriptor, alias, backing) = derived_account();
            let other = AccountId32::new([45; 32]);
            match role {
                "coldkey" => {
                    let _ = SubtensorModule::create_account_if_non_existent(&backing, &other);
                }
                "hotkey" => {
                    let _ = SubtensorModule::create_account_if_non_existent(&other, &backing);
                }
                _ => pallet_subtensor::HotkeySwapBasketCredits::<Runtime>::insert(
                    &backing,
                    &other,
                    (),
                ),
            }
            let state = System::account(&backing);
            assert_eq!(state.nonce, 0);
            assert_eq!(
                (state.providers, state.consumers, state.sufficients),
                (0, 0, 0)
            );
            let before = sp_io::storage::root(sp_runtime::StateVersion::V1);
            assert_eq!(
                HashedAccounts::register_derived(RuntimeOrigin::signed(parent), descriptor),
                Err(DispatchError::Other("HashedEvmAliasAlreadyUsed")),
                "{role}",
            );
            assert_eq!(
                sp_io::storage::root(sp_runtime::StateVersion::V1),
                before,
                "{role}"
            );
            assert_eq!(Mapping::into_account_id(alias), backing);
        });
    }
}

#[test]
fn fresh_alias_registration_still_sweeps_only_tao_and_is_idempotent() {
    for prefunded in [false, true] {
        ext().execute_with(|| {
            let (parent, descriptor, alias, backing) = derived_account();
            let owner = descriptor.account_id();
            if prefunded {
                fund(&backing);
            }
            let donation = Balances::free_balance(&backing);
            let reserved = Balances::reserved_balance(&parent);
            assert_ok!(HashedAccounts::register_derived(
                RuntimeOrigin::signed(parent.clone()),
                descriptor.clone(),
            ));
            assert_eq!(Mapping::into_account_id(alias), owner);
            assert_eq!(Balances::free_balance(&owner), donation);
            assert_eq!(Balances::free_balance(&backing), TaoBalance::new(0));
            assert_eq!(
                Balances::reserved_balance(&parent),
                reserved.saturating_add(node_subtensor_runtime::DerivedRegistrationDeposit::get()),
            );
            assert_eq!(System::account(&backing).nonce, 1);
            assert_eq!(System::account(&backing).sufficients, 1);
            let before = sp_io::storage::root(sp_runtime::StateVersion::V1);
            assert_ok!(HashedAccounts::register_derived(
                RuntimeOrigin::signed(parent),
                descriptor,
            ));
            assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), before);
        });
    }
}

#[test]
fn failed_registration_deposit_rolls_back_alias_preparation_and_tao_sweep() {
    ext().execute_with(|| {
        let (parent, descriptor, alias, backing) = derived_account();
        fund(&backing);
        assert_ok!(Balances::force_set_balance(
            RuntimeOrigin::root(),
            parent.clone().into(),
            TaoBalance::new(0),
        ));
        let before = sp_io::storage::root(sp_runtime::StateVersion::V1);
        assert!(
            HashedAccounts::register_derived(RuntimeOrigin::signed(parent), descriptor).is_err()
        );
        assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), before);
        assert_eq!(Mapping::into_account_id(alias), backing);
    });
}

#[test]
fn rotating_pq_registrations_share_the_backing_stake_guard() {
    for algorithm in [
        subtensor_hashed::mldsa::ALGORITHM,
        subtensor_hashed::falcon::ALGORITHM_512,
        subtensor_hashed::falcon::ALGORITHM_1024,
    ] {
        ext().execute_with(|| {
            let sponsor = AccountId32::new([46; 32]);
            fund(&sponsor);
            let descriptor = subtensor_hashed::mldsa::Descriptor {
                version: 2,
                algorithm,
                initial_commitment: [47; 64],
            };
            let account = AccountId32::new(subtensor_hashed::mldsa::account_id(&descriptor));
            let alias = H160::from_slice(&AsRef::<[u8; 32]>::as_ref(&account)[..20]);
            let backing = pallet_evm::HashedAddressMapping::<BlakeTwo256>::into_account_id(alias);
            transfer_incoming_stake(&backing);
            let registration = if algorithm == subtensor_hashed::mldsa::ALGORITHM {
                RegistrationDescriptor::MlDsa65(descriptor)
            } else {
                RegistrationDescriptor::Falcon(descriptor)
            };
            let before = sp_io::storage::root(sp_runtime::StateVersion::V1);
            assert_eq!(
                HashedAccounts::register(RuntimeOrigin::signed(sponsor), registration),
                Err(DispatchError::Other("HashedEvmAliasAlreadyUsed")),
            );
            assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), before);
            assert_eq!(Mapping::into_account_id(alias), backing);
        });
    }
}

#[test]
fn alias_registration_rejects_reaped_evm_accounts_with_crowdloan_contributions() {
    ext().execute_with(|| {
        let (parent, descriptor, alias, backing) = derived_account();
        let owner = descriptor.account_id();
        let creator = AccountId32::new([48; 32]);
        fund(&creator);
        assert_ok!(node_subtensor_runtime::Crowdloan::create(
            RuntimeOrigin::signed(creator),
            node_subtensor_runtime::MinimumDeposit::get(),
            node_subtensor_runtime::AbsoluteMinimumContribution::get(),
            TaoBalance::new(100_000_000_000),
            System::block_number() + node_subtensor_runtime::MinimumBlockDuration::get(),
            None,
            Some(AccountId32::new([49; 32])),
        ));

        fund(&backing);
        pallet_evm::DisableWhitelistCheck::<Runtime>::put(true);
        let call = |input| {
            assert_ok!(EVM::call(
                RuntimeOrigin::signed(owner.clone()),
                alias,
                H160::from_low_u64_be(2057),
                input,
                U256::zero(),
                1_000_000,
                <Runtime as pallet_evm::Config>::FeeCalculator::min_gas_price().0,
                None,
                None,
                Vec::new(),
                Vec::new(),
            ));
        };
        let amount = TaoBalance::new(1_000_000_000);
        let mut input = sp_io::hashing::keccak_256(b"contribute(uint32,uint64)")[..4].to_vec();
        input.extend(U256::zero().to_big_endian());
        input.extend(U256::from(u64::from(amount)).to_big_endian());
        call(input);
        assert_eq!(
            pallet_crowdloan::Contributions::<Runtime>::get(0, &backing),
            Some(amount)
        );

        // A real withdrawal reaps System state, but the separate EVM nonce and
        // the refundable contribution survive. System nonce zero is not fresh.
        assert_ok!(EVM::withdraw(
            RuntimeOrigin::signed(owner.clone()),
            alias,
            Balances::free_balance(&backing),
        ));
        assert_eq!(System::account(&backing), Default::default());
        assert!(pallet_evm::AccountNonces::<Runtime>::get(&backing) > 0);
        let before = sp_io::storage::root(sp_runtime::StateVersion::V1);
        assert_eq!(
            HashedAccounts::register_derived(RuntimeOrigin::signed(parent), descriptor),
            Err(DispatchError::Other("HashedEvmAliasAlreadyUsed")),
        );
        assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), before);
        assert_eq!(Mapping::into_account_id(alias), backing);

        fund(&backing);
        let balance_before = Balances::free_balance(&backing);
        let mut input = sp_io::hashing::keccak_256(b"withdraw(uint32)")[..4].to_vec();
        input.extend(U256::zero().to_big_endian());
        call(input);
        assert!(!pallet_crowdloan::Contributions::<Runtime>::contains_key(
            0, &backing
        ));
        assert!(Balances::free_balance(&backing) > balance_before);
    });
}

#[test]
fn registered_owner_recovers_delayed_crowdloan_payout_from_legacy_backing() {
    ext().execute_with(|| {
        let (parent, descriptor, alias, backing) = derived_account();
        let owner = descriptor.account_id();
        let creator = AccountId32::new([50; 32]);
        let creator_alias = H160::from_slice(&AsRef::<[u8; 32]>::as_ref(&creator)[..20]);
        let creator_backing = Mapping::into_account_id(creator_alias);
        fund(&creator_backing);
        pallet_evm::DisableWhitelistCheck::<Runtime>::put(true);

        let cap = TaoBalance::new(100_000_000_000);
        let deposit = node_subtensor_runtime::MinimumDeposit::get();
        let end = System::block_number() + node_subtensor_runtime::MinimumBlockDuration::get();
        let mut input = sp_io::hashing::keccak_256(b"create(uint64,uint64,uint64,uint32,address)")
            [..4]
            .to_vec();
        for value in [
            U256::from(u64::from(deposit)),
            U256::from(u64::from(
                node_subtensor_runtime::AbsoluteMinimumContribution::get(),
            )),
            U256::from(u64::from(cap)),
            U256::from(end),
        ] {
            input.extend(value.to_big_endian());
        }
        input.extend([0u8; 12]);
        input.extend(alias.as_bytes());
        assert_ok!(EVM::call(
            RuntimeOrigin::signed(creator),
            creator_alias,
            H160::from_low_u64_be(2057),
            input,
            U256::zero(),
            1_000_000,
            <Runtime as pallet_evm::Config>::FeeCalculator::min_gas_price().0,
            None,
            None,
            Vec::new(),
            Vec::new(),
        ));
        assert_eq!(
            pallet_crowdloan::Crowdloans::<Runtime>::get(0)
                .unwrap()
                .target_address,
            Some(backing.clone()),
        );
        assert_eq!(System::account(&backing), Default::default());

        assert_ok!(HashedAccounts::register_derived(
            RuntimeOrigin::signed(parent),
            descriptor,
        ));
        assert_eq!(Mapping::into_account_id(alias), owner);
        assert_ok!(node_subtensor_runtime::Crowdloan::contribute(
            RuntimeOrigin::signed(creator_backing.clone()),
            0,
            cap.saturating_sub(deposit),
        ));
        assert_ok!(node_subtensor_runtime::Crowdloan::finalize(
            RuntimeOrigin::signed(creator_backing),
            0,
        ));
        assert_eq!(Balances::free_balance(&backing), cap);
        assert_eq!(Balances::free_balance(&owner), TaoBalance::new(0));

        // An unrelated native account with the same EVM prefix cannot sweep it.
        let mut impostor = *AsRef::<[u8; 32]>::as_ref(&owner);
        impostor[31] ^= 1;
        assert_eq!(
            EVM::withdraw(
                RuntimeOrigin::signed(AccountId32::new(impostor)),
                alias,
                cap
            ),
            Err(DispatchError::BadOrigin),
        );
        assert_eq!(Balances::free_balance(&backing), cap);

        assert_ok!(EVM::withdraw(
            RuntimeOrigin::signed(owner.clone()),
            alias,
            cap
        ));
        assert_eq!(Balances::free_balance(&owner), cap);
        assert_eq!(Balances::free_balance(&backing), TaoBalance::new(0));
        assert_eq!(Mapping::into_account_id(alias), owner);
    });
}
