#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::arithmetic_side_effects)]

//! Staking-precompile stake exits reserve the `StakingHotkeys` walk for the signer's current
//! list, not the cap, so a normal EVM gas limit covers them. Runs on the real runtime because
//! its database weights are what make the capped walk (~2.3M gas) exceed such a limit.

use fp_evm::{Context, ExitError, PrecompileFailure};
use frame_support::dispatch::GetDispatchInfo;
use node_subtensor_runtime::{BuildStorage, Runtime, RuntimeCall, RuntimeGenesisConfig, System};
use pallet_evm::{AddressMapping, GasWeightMapping, PrecompileSet};
use pallet_subtensor::weights::WeightInfo;
use precompile_utils::solidity::encode_with_selector;
use precompile_utils::testing::MockHandle;
use sp_core::{H160, H256, U256};
use sp_runtime::{Weight, traits::Zero};
use substrate_fixed::types::U64F64;
use subtensor_precompiles::{PrecompileExt, Precompiles, StakingPrecompileV2};
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance};

type AccountId = <Runtime as frame_system::Config>::AccountId;
type Subtensor = pallet_subtensor::Pallet<Runtime>;
type SubtensorWeights = <Runtime as pallet_subtensor::Config>::WeightInfo;

/// The gas limit stake exits fit in before the walk was declared at the cap.
const GAS_LIMIT: u64 = 300_000;
const NETUID: u16 = 1;
const STAKE_RAO: u64 = 20_000_000_000;
const EXIT_RAO: u64 = 1_000_000_000;

fn selector(signature: &str) -> u32 {
    let hash = sp_io::hashing::keccak_256(signature.as_bytes());
    u32::from_be_bytes([hash[0], hash[1], hash[2], hash[3]])
}

fn gas(weight: Weight) -> u64 {
    <Runtime as pallet_evm::Config>::GasWeightMapping::weight_to_gas(weight)
}

fn account(address: H160) -> AccountId {
    <Runtime as pallet_evm::Config>::AddressMapping::into_account_id(address)
}

fn h256(account: &AccountId) -> H256 {
    H256::from_slice(account.as_ref())
}

/// Calls the V2 staking precompile and returns the result with the gas it recorded.
fn call(caller: H160, input: Vec<u8>, gas_limit: u64) -> (Result<(), PrecompileFailure>, u64) {
    let address = H160::from_low_u64_be(StakingPrecompileV2::<Runtime>::INDEX);
    let mut handle = MockHandle::new(
        address,
        Context {
            address,
            caller,
            apparent_value: U256::zero(),
        },
    );
    handle.input = input;
    handle.gas_limit = gas_limit;
    let result = Precompiles::<Runtime>::new()
        .execute(&mut handle)
        .expect("routed to the staking precompile");
    (result.map(|_| ()), handle.gas_used)
}

fn remove_stake(hotkey: &AccountId) -> Vec<u8> {
    encode_with_selector(
        selector("removeStake(bytes32,uint256,uint256)"),
        (h256(hotkey), U256::from(EXIT_RAO), U256::from(NETUID)),
    )
}

fn staking_hotkeys(coldkey: &AccountId) -> usize {
    pallet_subtensor::StakingHotkeys::<Runtime>::get(coldkey).len()
}

/// Pads `coldkey`'s `StakingHotkeys` to `MAX_STAKING_HOTKEYS` with unrelated hotkeys.
fn fill_staking_hotkeys(coldkey: &AccountId) {
    let mut hotkeys = pallet_subtensor::StakingHotkeys::<Runtime>::get(coldkey);
    let mut index = 0_u64;
    while hotkeys.len() < pallet_subtensor::MAX_STAKING_HOTKEYS as usize {
        let mut raw = [0xd0_u8; 32];
        raw[..8].copy_from_slice(&index.to_le_bytes());
        hotkeys.push(AccountId::from(raw));
        index += 1;
    }
    pallet_subtensor::StakingHotkeys::<Runtime>::insert(coldkey, hotkeys);
}

/// A subnet with `hotkey` registered as an owner and `caller` staked to it.
fn new_test_ext(caller: H160, hotkey: &AccountId) -> sp_io::TestExternalities {
    let mut ext: sp_io::TestExternalities = RuntimeGenesisConfig::default()
        .build_storage()
        .unwrap()
        .into();
    ext.execute_with(|| {
        System::set_block_number(1);
        let netuid = NetUid::from(NETUID);
        Subtensor::init_new_network(netuid, 100);
        pallet_subtensor::FirstEmissionBlockNumber::<Runtime>::insert(netuid, 0);
        pallet_subtensor::SubtokenEnabled::<Runtime>::insert(netuid, true);
        pallet_subtensor::TransferToggle::<Runtime>::insert(netuid, true);
        pallet_subtensor::BurnIncreaseMult::<Runtime>::insert(netuid, U64F64::from_num(1));
        pallet_subtensor::SubnetTAO::<Runtime>::insert(
            netuid,
            TaoBalance::from(200_000_000_000_u64),
        );
        pallet_subtensor::SubnetAlphaIn::<Runtime>::insert(
            netuid,
            AlphaBalance::from(100_000_000_000_u64),
        );
        pallet_subtensor::Owner::<Runtime>::insert(hotkey, hotkey.clone());

        let balance = TaoBalance::from(100_000_000_000_u64);
        let credit = Subtensor::mint_tao(balance);
        let _ = Subtensor::spend_tao(&account(caller), credit, balance).unwrap();
        let (result, _) = call(
            caller,
            encode_with_selector(
                selector("addStake(bytes32,uint256,uint256)"),
                (h256(hotkey), U256::from(STAKE_RAO), U256::from(NETUID)),
            ),
            u64::MAX,
        );
        result.expect("addStake");
        System::set_block_number(10_000);
    });
    ext
}

#[test]
fn stake_exits_fit_a_normal_gas_limit_for_a_short_list() {
    let caller = H160::from_low_u64_be(0x5a01);
    let hotkey = AccountId::from([0x11; 32]);
    let destination = AccountId::from([0x99; 32]);
    new_test_ext(caller, &hotkey).execute_with(|| {
        let coldkey = account(caller);
        let netuid = NetUid::from(NETUID);
        let amount = AlphaBalance::from(EXIT_RAO);

        // The declared (transaction) weight still carries the capped walk, beyond this limit.
        let declared = RuntimeCall::SubtensorModule(pallet_subtensor::Call::transfer_stake {
            destination_coldkey: destination.clone(),
            hotkey: hotkey.clone(),
            origin_netuid: netuid,
            destination_netuid: netuid,
            alpha_amount: amount,
        })
        .get_dispatch_info()
        .total_weight();
        assert!(gas(declared) > GAS_LIMIT);

        let (result, used) = call(
            caller,
            encode_with_selector(
                selector("transferStake(bytes32,bytes32,uint256,uint256,uint256)"),
                (
                    h256(&destination),
                    h256(&hotkey),
                    U256::from(NETUID),
                    U256::from(NETUID),
                    U256::from(EXIT_RAO),
                ),
            ),
            GAS_LIMIT,
        );
        assert_eq!(result, Ok(()));
        assert!(
            !Subtensor::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &destination, netuid)
                .is_zero()
        );
        // Charged at least the benchmarked call and the walk over the caller's list.
        let actual = SubtensorWeights::transfer_stake()
            .saturating_add(Subtensor::staking_hotkeys_walk_actual(&coldkey));
        assert!(used >= gas(actual), "used {used} < {}", gas(actual));

        let (result, _) = call(caller, remove_stake(&hotkey), GAS_LIMIT);
        assert_eq!(result, Ok(()));
    });
}

#[test]
fn moving_to_a_new_hotkey_fits_the_reserved_walk() {
    let caller = H160::from_low_u64_be(0x5a02);
    let hotkey = AccountId::from([0x11; 32]);
    let new_hotkey = AccountId::from([0x77; 32]);
    new_test_ext(caller, &hotkey).execute_with(|| {
        let coldkey = account(caller);
        pallet_subtensor::Owner::<Runtime>::insert(&new_hotkey, new_hotkey.clone());
        let before = staking_hotkeys(&coldkey);

        let (result, used) = call(
            caller,
            encode_with_selector(
                selector("moveStake(bytes32,bytes32,uint256,uint256,uint256)"),
                (
                    h256(&hotkey),
                    h256(&new_hotkey),
                    U256::from(NETUID),
                    U256::from(NETUID),
                    U256::from(EXIT_RAO),
                ),
            ),
            GAS_LIMIT,
        );
        assert_eq!(result, Ok(()));
        // The call grew the list it walks; the post-dispatch walk is still within the
        // reservation, so it is charged in full rather than capped.
        assert_eq!(staking_hotkeys(&coldkey), before + 1);
        let actual = SubtensorWeights::move_stake()
            .saturating_add(Subtensor::staking_hotkeys_walk_actual(&coldkey));
        assert!(used >= gas(actual), "used {used} < {}", gas(actual));
    });
}

#[test]
fn a_capped_list_still_needs_the_full_walk_allowance() {
    let caller = H160::from_low_u64_be(0x5a03);
    let hotkey = AccountId::from([0x11; 32]);
    new_test_ext(caller, &hotkey).execute_with(|| {
        fill_staking_hotkeys(&account(caller));

        let (result, _) = call(caller, remove_stake(&hotkey), GAS_LIMIT);
        assert_eq!(
            result,
            Err(PrecompileFailure::Error {
                exit_status: ExitError::OutOfGas
            })
        );

        let walk = gas(Subtensor::staking_hotkeys_walk_bound());
        let (result, used) = call(caller, remove_stake(&hotkey), walk + 1_000_000);
        assert_eq!(result, Ok(()));
        assert!(used >= walk, "used {used} < {walk}");
    });
}

/// `unstakeAll` and `unstakeAllAlpha` declare a walk at the cap on each of their 16 envelope
/// slots (~37M gas); sized to the caller's list they fit a few million.
#[test]
fn unstake_all_reserves_every_slot_walk_for_the_callers_list() {
    const UNSTAKE_ALL_GAS_LIMIT: u64 = 3_000_000;
    for (signature, alpha) in [
        ("unstakeAll(bytes32)", false),
        ("unstakeAllAlpha(bytes32)", true),
    ] {
        let caller = H160::from_low_u64_be(0x5a10);
        let hotkey = AccountId::from([0x11; 32]);
        new_test_ext(caller, &hotkey).execute_with(|| {
            // Enough subnets that every envelope slot is declared.
            for netuid in 2..=24_u16 {
                Subtensor::init_new_network(NetUid::from(netuid), 100);
            }
            let coldkey = account(caller);
            let declared = if alpha {
                Subtensor::unstake_all_alpha_declared_weight()
            } else {
                Subtensor::unstake_all_declared_weight()
            };
            assert!(gas(declared) > 10 * UNSTAKE_ALL_GAS_LIMIT);
            let input = || encode_with_selector(selector(signature), (h256(&hotkey),));

            // At the cap every slot still needs its full walk.
            let short_list = pallet_subtensor::StakingHotkeys::<Runtime>::get(&coldkey);
            fill_staking_hotkeys(&coldkey);
            let (result, _) = call(caller, input(), UNSTAKE_ALL_GAS_LIMIT);
            assert_eq!(
                result,
                Err(PrecompileFailure::Error {
                    exit_status: ExitError::OutOfGas
                }),
                "{signature}"
            );
            pallet_subtensor::StakingHotkeys::<Runtime>::insert(&coldkey, short_list);

            let (result, used) = call(caller, input(), UNSTAKE_ALL_GAS_LIMIT);
            assert_eq!(result, Ok(()), "{signature}");
            assert!(
                Subtensor::get_stake_for_hotkey_and_coldkey_on_subnet(
                    &hotkey,
                    &coldkey,
                    NetUid::from(NETUID)
                )
                .is_zero()
            );
            // Charged at least the one unstaked position and its walk (not capped by the
            // reservation); skipped empty subnets only add to this.
            let work = pallet_subtensor::staking::remove_stake::UnstakeAllWork {
                scanned: 1,
                legs: 1,
                validated: 1,
            };
            let actual = if alpha {
                Subtensor::unstake_all_alpha_actual_weight(&coldkey, work)
            } else {
                Subtensor::unstake_all_actual_weight(&coldkey, work)
            };
            assert!(
                used >= gas(actual),
                "{signature}: used {used} < {}",
                gas(actual)
            );
        });
    }
}
