#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used)]

use core::marker::PhantomData;
use frame_benchmarking::{BenchmarkError, account, v2::*};
use pallet_multi_collective::Pallet as MultiCollective;
use pallet_subtensor::{
    Pallet as Subtensor,
    root_registered::{EmaValueProvider, SampleStep},
    *,
};
use sp_std::vec::Vec;
use substrate_fixed::types::{I96F32, U64F64};
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance};

use super::{
    BUILDING_SIZE, CollectiveId, ECONOMIC_ELIGIBILITY_THRESHOLD, ECONOMIC_ELIGIBLE_SIZE,
    ECONOMIC_SIZE, MIN_SUBNET_AGE, STAKE_CHUNK_SUBNETS, STAKE_VALUE_HOTKEYS, StakeValueProgress,
    StakeValueProvider, TermManagement,
};
use crate::{AccountId, Runtime};

pub trait Config: frame_system::Config {}

pub struct Pallet<T: Config>(PhantomData<T>);

impl Config for Runtime {}

const FIRST_BENCHMARK_NETUID: u16 = 1024;
const BUILDING_BENCHMARK_SUBNETS: u32 = 128;

#[benchmarks]
mod benchmarks {
    use super::*;

    #[benchmark]
    fn stake_ema_provider_step() -> Result<(), BenchmarkError> {
        let (coldkey, progress) = prepare_stake_value_state();
        let expected_offset = progress.subnet_offset.saturating_add(STAKE_CHUNK_SUBNETS);
        let result;

        #[block]
        {
            result = StakeValueProvider::step(&coldkey, progress);
        }

        assert!(matches!(
            result.0,
            SampleStep::Continue { progress }
                if progress.subnet_offset == expected_offset && progress.accumulated_tao > 0
        ));

        Ok(())
    }

    #[benchmark]
    fn rotate_economic() -> Result<(), BenchmarkError> {
        let expected = expected_stored_members(prepare_economic_rotation_state());

        #[block]
        {
            let _ = TermManagement::rotate_economic();
        }

        assert_eq!(members_of(CollectiveId::Economic), expected);

        Ok(())
    }

    #[benchmark]
    fn rotate_building() -> Result<(), BenchmarkError> {
        let expected = expected_stored_members(prepare_building_rotation_state());

        #[block]
        {
            let _ = TermManagement::rotate_building();
        }

        assert_eq!(members_of(CollectiveId::Building), expected);

        Ok(())
    }

    fn seed_swap_reserves(netuid: NetUid) {
        SubnetTAO::<Runtime>::insert(netuid, TaoBalance::from(150_000_000_000_u64));
        SubnetAlphaIn::<Runtime>::insert(netuid, AlphaBalance::from(100_000_000_000_u64));
    }

    fn add_balance_to_coldkey_account(coldkey: &AccountId, tao: TaoBalance) {
        let credit = Subtensor::<Runtime>::mint_tao(tao);
        let _ = Subtensor::<Runtime>::spend_tao(coldkey, credit, tao).unwrap();
    }

    fn prepare_stake_value_state() -> (AccountId, StakeValueProgress) {
        let coldkey: AccountId = account("StakeValueColdkey", 0, 0);
        add_balance_to_coldkey_account(&coldkey, TaoBalance::from(1_000_000_000_u64));

        let mut hotkeys: Vec<AccountId> = Vec::with_capacity(STAKE_VALUE_HOTKEYS as usize);
        for hotkey_index in 0..STAKE_VALUE_HOTKEYS {
            hotkeys.push(account("StakeValueHotkey", hotkey_index, 0));
        }
        OwnedHotkeys::<Runtime>::insert(&coldkey, hotkeys.clone());

        let mut first_netuid = None;
        for subnet_index in 0..STAKE_CHUNK_SUBNETS {
            let netuid = NetUid::from(FIRST_BENCHMARK_NETUID.saturating_add(subnet_index as u16));
            if first_netuid.is_none() {
                first_netuid = Some(netuid);
            }

            Subtensor::<Runtime>::init_new_network(netuid, 1);
            SubtokenEnabled::<Runtime>::insert(netuid, true);
            seed_swap_reserves(netuid);

            for hotkey in &hotkeys {
                TotalHotkeyAlpha::<Runtime>::insert(
                    hotkey.clone(),
                    netuid,
                    AlphaBalance::from(1_000_000_000_u64),
                );
            }
        }

        let netuids = Subtensor::<Runtime>::get_all_subnet_netuids();
        let subnet_offset = netuids
            .iter()
            .position(|netuid| Some(*netuid) == first_netuid)
            .unwrap_or_default() as u32;

        (
            coldkey,
            StakeValueProgress {
                subnet_offset,
                accumulated_tao: 0,
            },
        )
    }

    fn set_members(collective_id: CollectiveId, members: Vec<AccountId>) {
        MultiCollective::<Runtime>::set_members(
            frame_system::RawOrigin::Root.into(),
            collective_id,
            members,
        )
        .unwrap();
    }

    fn members_of(collective_id: CollectiveId) -> Vec<AccountId> {
        <MultiCollective<Runtime> as pallet_multi_collective::CollectiveInspect<
            AccountId,
            CollectiveId,
        >>::members_of(collective_id)
    }

    fn expected_stored_members(mut members: Vec<AccountId>) -> Vec<AccountId> {
        members.sort();
        members
    }

    fn prepare_economic_rotation_state() -> Vec<AccountId> {
        let eligible = (0..ECONOMIC_ELIGIBLE_SIZE)
            .map(|index| {
                let coldkey = account("EconomicEligibleColdkey", index, 0);
                RootRegisteredEma::<Runtime>::insert(
                    &coldkey,
                    pallet_subtensor::root_registered::EmaState {
                        ema: U64F64::from_num(ECONOMIC_ELIGIBLE_SIZE - index),
                        samples: ECONOMIC_ELIGIBILITY_THRESHOLD,
                    },
                );
                coldkey
            })
            .collect::<Vec<_>>();
        set_members(CollectiveId::EconomicEligible, eligible);

        let old_members = (0..ECONOMIC_SIZE)
            .map(|index| account("OldEconomicMember", index, 0))
            .collect::<Vec<_>>();
        set_members(CollectiveId::Economic, old_members);

        TermManagement::top_validators(ECONOMIC_SIZE).0
    }

    fn prepare_building_rotation_state() -> Vec<AccountId> {
        frame_system::Pallet::<Runtime>::set_block_number(MIN_SUBNET_AGE.saturating_add(1));

        let old_members = (0..BUILDING_SIZE)
            .map(|index| account("OldBuildingMember", index, 0))
            .collect::<Vec<_>>();
        set_members(CollectiveId::Building, old_members);

        for subnet_index in 0..BUILDING_BENCHMARK_SUBNETS {
            let netuid = NetUid::from(4_096_u16.saturating_add(subnet_index as u16));
            let owner_index = subnet_index % BUILDING_SIZE;
            let owner: AccountId = account("BuildingOwner", owner_index, 0);

            Subtensor::<Runtime>::init_new_network(netuid, 1);
            NetworkRegisteredAt::<Runtime>::insert(netuid, 0);
            SubnetOwner::<Runtime>::insert(netuid, owner);
            SubnetMovingPrice::<Runtime>::insert(
                netuid,
                I96F32::from_num(BUILDING_BENCHMARK_SUBNETS - subnet_index),
            );
        }

        TermManagement::top_subnet_owners(BUILDING_SIZE, MIN_SUBNET_AGE).0
    }
}
