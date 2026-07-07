use alloc::{boxed::Box, string::String};
use core::marker::PhantomData;

use fp_evm::{ExitError, PrecompileFailure};
use frame_support::dispatch::{DispatchInfo, GetDispatchInfo, PostDispatchInfo};
use frame_support::traits::IsSubType;
use frame_system::RawOrigin;
use pallet_evm::AddressMapping;
use pallet_evm::PrecompileHandle;
use precompile_utils::{EvmResult, solidity::Codec};
use sp_core::{ByteArray, H256};
use sp_runtime::{
    Percent,
    traits::{AsSystemOriginSigner, Dispatchable, UniqueSaturatedInto},
};
use subtensor_runtime_common::NetUid;

use crate::{PrecompileExt, PrecompileHandleExt};

pub struct LeasingPrecompile<R>(PhantomData<R>);

impl<R> PrecompileExt<R::AccountId> for LeasingPrecompile<R>
where
    R: frame_system::Config
        + pallet_balances::Config
        + pallet_evm::Config
        + pallet_subtensor::Config
        + pallet_crowdloan::Config
        + pallet_shield::Config
        + pallet_subtensor_proxy::Config
        + Send
        + Sync
        + scale_info::TypeInfo,
    R::AccountId: From<[u8; 32]> + ByteArray,
    <R as frame_system::Config>::RuntimeOrigin: AsSystemOriginSigner<R::AccountId> + Clone,
    <R as frame_system::Config>::RuntimeCall: From<pallet_subtensor::Call<R>>
        + From<pallet_crowdloan::Call<R>>
        + GetDispatchInfo
        + Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>
        + IsSubType<pallet_balances::Call<R>>
        + IsSubType<pallet_subtensor::Call<R>>
        + IsSubType<pallet_shield::Call<R>>
        + IsSubType<pallet_subtensor_proxy::Call<R>>,
    <R as pallet_evm::Config>::AddressMapping: AddressMapping<R::AccountId>,
{
    const INDEX: u64 = 2058;
}

#[precompile_utils::precompile]
impl<R> LeasingPrecompile<R>
where
    R: frame_system::Config
        + pallet_balances::Config
        + pallet_evm::Config
        + pallet_subtensor::Config
        + pallet_crowdloan::Config
        + pallet_shield::Config
        + pallet_subtensor_proxy::Config
        + Send
        + Sync
        + scale_info::TypeInfo,
    R::AccountId: From<[u8; 32]> + ByteArray,
    <R as frame_system::Config>::RuntimeOrigin: AsSystemOriginSigner<R::AccountId> + Clone,
    <R as frame_system::Config>::RuntimeCall: From<pallet_subtensor::Call<R>>
        + From<pallet_crowdloan::Call<R>>
        + GetDispatchInfo
        + Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>
        + IsSubType<pallet_balances::Call<R>>
        + IsSubType<pallet_subtensor::Call<R>>
        + IsSubType<pallet_shield::Call<R>>
        + IsSubType<pallet_subtensor_proxy::Call<R>>,
    <R as pallet_evm::Config>::AddressMapping: AddressMapping<R::AccountId>,
{
    #[precompile::public("getLease(uint32)")]
    #[precompile::view]
    fn get_lease(handle: &mut impl PrecompileHandle, lease_id: u32) -> EvmResult<LeaseInfo> {
        handle.record_db_reads::<R>(1)?;
        let lease =
            pallet_subtensor::SubnetLeases::<R>::get(lease_id).ok_or(PrecompileFailure::Error {
                exit_status: ExitError::Other("Lease not found".into()),
            })?;

        Ok(LeaseInfo {
            beneficiary: H256::from_slice(lease.beneficiary.as_slice()),
            coldkey: H256::from_slice(lease.coldkey.as_slice()),
            hotkey: H256::from_slice(lease.hotkey.as_slice()),
            emissions_share: lease.emissions_share.deconstruct(),
            has_end_block: lease.end_block.is_some(),
            end_block: lease
                .end_block
                .map(|b| b.unique_saturated_into())
                .unwrap_or(0),
            netuid: lease.netuid.into(),
            cost: lease.cost.into(),
        })
    }

    #[precompile::public("getContributorShare(uint32,bytes32)")]
    #[precompile::view]
    fn get_contributor_share(
        handle: &mut impl PrecompileHandle,
        lease_id: u32,
        contributor: H256,
    ) -> EvmResult<(u128, u128)> {
        handle.record_db_reads::<R>(1)?;
        let contributor = R::AccountId::from(contributor.0);
        let share = pallet_subtensor::SubnetLeaseShares::<R>::get(lease_id, contributor);

        Ok((share.int().to_bits(), share.frac().to_bits()))
    }

    #[precompile::public("getLeaseIdForSubnet(uint16)")]
    #[precompile::view]
    fn get_lease_id_for_subnet(handle: &mut impl PrecompileHandle, netuid: u16) -> EvmResult<u32> {
        handle.record_db_reads::<R>(1)?;
        let lease_id = pallet_subtensor::SubnetUidToLeaseId::<R>::get(NetUid::from(netuid)).ok_or(
            PrecompileFailure::Error {
                exit_status: ExitError::Other("Lease not found for netuid".into()),
            },
        )?;

        Ok(lease_id.into())
    }

    #[precompile::public("createLeaseCrowdloan(uint64,uint64,uint64,uint32,uint8,bool,uint32)")]
    #[precompile::payable]
    #[allow(clippy::too_many_arguments)]
    fn create_lease_crowdloan(
        handle: &mut impl PrecompileHandle,
        crowdloan_deposit: u64,
        crowdloan_min_contribution: u64,
        crowdloan_cap: u64,
        crowdloan_end: u32,
        leasing_emissions_share: u8,
        has_leasing_end_block: bool,
        leasing_end_block: u32,
    ) -> EvmResult<()> {
        let who = handle.caller_account_id::<R>();

        let leasing_end_block = if has_leasing_end_block {
            Some(leasing_end_block.into())
        } else {
            None
        };

        let leasing_call = {
            let call = pallet_subtensor::Call::<R>::register_leased_network {
                emissions_share: Percent::from_percent(leasing_emissions_share),
                end_block: leasing_end_block,
            };
            let system_call: <R as frame_system::Config>::RuntimeCall = call.into();
            Box::new(system_call.into())
        };

        let crowdloan_call = pallet_crowdloan::Call::<R>::create {
            deposit: crowdloan_deposit.into(),
            min_contribution: crowdloan_min_contribution.into(),
            cap: crowdloan_cap.into(),
            end: crowdloan_end.into(),
            call: Some(leasing_call),
            target_address: None,
        };

        handle.try_dispatch_runtime_call::<R, _>(crowdloan_call, RawOrigin::Signed(who))
    }

    #[precompile::public("terminateLease(uint32,bytes32)")]
    #[precompile::payable]
    fn terminate_lease(
        handle: &mut impl PrecompileHandle,
        lease_id: u32,
        hotkey: H256,
    ) -> EvmResult<()> {
        let who = handle.caller_account_id::<R>();
        let hotkey = R::AccountId::from(hotkey.0);
        let call = pallet_subtensor::Call::<R>::terminate_lease { lease_id, hotkey };

        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(who))
    }
}

#[derive(Codec)]
struct LeaseInfo {
    beneficiary: H256,
    coldkey: H256,
    hotkey: H256,
    emissions_share: u8,
    has_end_block: bool,
    end_block: u32,
    netuid: u16,
    cost: u64,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::arithmetic_side_effects)]

    use super::*;
    use crate::PrecompileExt;
    use crate::mock::{
        AccountId, Runtime, RuntimeCall, RuntimeOrigin, System, addr_from_index, fund_account,
        mapped_account, new_test_ext, precompiles, selector_u32,
    };
    use frame_support::StorageDoubleMap;
    use precompile_utils::solidity::{encode_return_value, encode_with_selector};
    use precompile_utils::testing::PrecompileTesterExt;
    use sp_core::H160;
    use subtensor_runtime_common::TaoBalance;

    const CROWDLOAN_DEPOSIT: u64 = 50;
    const CROWDLOAN_MIN_CONTRIBUTION: u64 = 10;
    const NETWORK_LOCK_COST: u64 = 100;
    const CROWDLOAN_CAP: u64 = 200;
    const CROWDLOAN_END: u32 = 50;
    const LEASING_EMISSIONS_SHARE: u8 = 15;
    const LEASING_END_BLOCK: u32 = 80;
    const ACCOUNT_BALANCE: u64 = 1_000;

    fn expected_lease_info(lease_id: u32) -> LeaseInfo {
        let lease =
            pallet_subtensor::SubnetLeases::<Runtime>::get(lease_id).expect("lease should exist");

        LeaseInfo {
            beneficiary: H256::from_slice(lease.beneficiary.as_slice()),
            coldkey: H256::from_slice(lease.coldkey.as_slice()),
            hotkey: H256::from_slice(lease.hotkey.as_slice()),
            emissions_share: lease.emissions_share.deconstruct(),
            has_end_block: lease.end_block.is_some(),
            end_block: lease.end_block.unwrap_or_default() as u32,
            netuid: lease.netuid.into(),
            cost: u64::from(lease.cost),
        }
    }

    fn get_lease(caller: H160, lease_id: u32, expected: LeaseInfo) {
        let precompile_addr = addr_from_index(LeasingPrecompile::<Runtime>::INDEX);

        precompiles::<LeasingPrecompile<Runtime>>()
            .prepare_test(
                caller,
                precompile_addr,
                encode_with_selector(selector_u32("getLease(uint32)"), (lease_id,)),
            )
            .with_static_call(true)
            .execute_returns_raw(encode_return_value(expected));
    }

    fn create_lease_crowdloan(caller: H160) -> u32 {
        let crowdloan_id = pallet_crowdloan::NextCrowdloanId::<Runtime>::get();
        let precompile_addr = addr_from_index(LeasingPrecompile::<Runtime>::INDEX);

        precompiles::<LeasingPrecompile<Runtime>>()
            .prepare_test(
                caller,
                precompile_addr,
                encode_with_selector(
                    selector_u32(
                        "createLeaseCrowdloan(uint64,uint64,uint64,uint32,uint8,bool,uint32)",
                    ),
                    (
                        CROWDLOAN_DEPOSIT,
                        CROWDLOAN_MIN_CONTRIBUTION,
                        CROWDLOAN_CAP,
                        CROWDLOAN_END,
                        LEASING_EMISSIONS_SHARE,
                        true,
                        LEASING_END_BLOCK,
                    ),
                ),
            )
            .execute_returns(());

        crowdloan_id
    }

    fn contribute_and_finalize(crowdloan_id: u32, creator: AccountId, contributor: AccountId) {
        pallet_crowdloan::Pallet::<Runtime>::contribute(
            RuntimeOrigin::signed(contributor),
            crowdloan_id,
            (CROWDLOAN_CAP - CROWDLOAN_DEPOSIT).into(),
        )
        .expect("contribute should work");

        System::set_block_number(CROWDLOAN_END.into());
        pallet_crowdloan::Pallet::<Runtime>::finalize(RuntimeOrigin::signed(creator), crowdloan_id)
            .expect("finalize should work");
    }

    fn set_leasing_fixture() {
        pallet_subtensor::NetworkMinLockCost::<Runtime>::set(TaoBalance::from(NETWORK_LOCK_COST));
        pallet_subtensor::NetworkLastLockCost::<Runtime>::set(TaoBalance::from(NETWORK_LOCK_COST));
    }

    #[test]
    fn leasing_precompile_reads_existing_pallet_lease_and_contributor_shares() {
        new_test_ext().execute_with(|| {
            set_leasing_fixture();

            let creator = AccountId::from([0x11; 32]);
            let contributor = AccountId::from([0x22; 32]);
            let caller = addr_from_index(0x8001);
            let crowdloan_id = pallet_crowdloan::NextCrowdloanId::<Runtime>::get();
            let lease_id = pallet_subtensor::NextSubnetLeaseId::<Runtime>::get();
            let leasing_call = pallet_subtensor::Call::<Runtime>::register_leased_network {
                emissions_share: Percent::from_percent(LEASING_EMISSIONS_SHARE),
                end_block: Some(LEASING_END_BLOCK.into()),
            };

            fund_account(&creator, ACCOUNT_BALANCE);
            fund_account(&contributor, ACCOUNT_BALANCE);
            pallet_crowdloan::Pallet::<Runtime>::create(
                RuntimeOrigin::signed(creator.clone()),
                CROWDLOAN_DEPOSIT.into(),
                CROWDLOAN_MIN_CONTRIBUTION.into(),
                CROWDLOAN_CAP.into(),
                CROWDLOAN_END.into(),
                Some(Box::new(RuntimeCall::from(leasing_call))),
                None,
            )
            .expect("direct crowdloan create should work");
            contribute_and_finalize(crowdloan_id, creator.clone(), contributor.clone());

            let lease = pallet_subtensor::SubnetLeases::<Runtime>::get(lease_id)
                .expect("lease should exist");
            get_lease(caller, lease_id, expected_lease_info(lease_id));

            let precompile_addr = addr_from_index(LeasingPrecompile::<Runtime>::INDEX);
            precompiles::<LeasingPrecompile<Runtime>>()
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("getLeaseIdForSubnet(uint16)"),
                        (u16::from(lease.netuid),),
                    ),
                )
                .with_static_call(true)
                .execute_returns(lease_id);

            precompiles::<LeasingPrecompile<Runtime>>()
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("getContributorShare(uint32,bytes32)"),
                        (lease_id, H256::from_slice(creator.as_slice())),
                    ),
                )
                .with_static_call(true)
                .execute_returns((0_u128, 0_u128));

            let contributor_share =
                pallet_subtensor::SubnetLeaseShares::<Runtime>::get(lease_id, &contributor);
            precompiles::<LeasingPrecompile<Runtime>>()
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("getContributorShare(uint32,bytes32)"),
                        (lease_id, H256::from_slice(contributor.as_slice())),
                    ),
                )
                .with_static_call(true)
                .execute_returns((
                    contributor_share.int().to_bits(),
                    contributor_share.frac().to_bits(),
                ));
        });
    }

    #[test]
    fn leasing_precompile_creates_lease_crowdloan_and_reads_created_lease() {
        new_test_ext().execute_with(|| {
            set_leasing_fixture();

            let creator = addr_from_index(0x8002);
            let contributor = addr_from_index(0x8003);
            let creator_account = mapped_account(creator);
            let contributor_account = mapped_account(contributor);
            let lease_id = pallet_subtensor::NextSubnetLeaseId::<Runtime>::get();

            fund_account(&creator_account, ACCOUNT_BALANCE);
            fund_account(&contributor_account, ACCOUNT_BALANCE);
            let crowdloan_id = create_lease_crowdloan(creator);
            contribute_and_finalize(
                crowdloan_id,
                creator_account.clone(),
                contributor_account.clone(),
            );

            let lease = pallet_subtensor::SubnetLeases::<Runtime>::get(lease_id)
                .expect("lease should exist");
            assert_eq!(lease.beneficiary, creator_account);
            assert_eq!(
                lease.emissions_share,
                Percent::from_percent(LEASING_EMISSIONS_SHARE)
            );
            assert_eq!(lease.end_block, Some(LEASING_END_BLOCK.into()));

            get_lease(creator, lease_id, expected_lease_info(lease_id));
            let contributor_share =
                pallet_subtensor::SubnetLeaseShares::<Runtime>::get(lease_id, &contributor_account);
            assert_ne!(
                (
                    contributor_share.int().to_bits(),
                    contributor_share.frac().to_bits()
                ),
                (0_u128, 0_u128),
            );
        });
    }

    #[test]
    fn leasing_precompile_terminates_ended_lease_and_transfers_subnet_ownership() {
        new_test_ext().execute_with(|| {
            set_leasing_fixture();

            let beneficiary = addr_from_index(0x8004);
            let contributor = addr_from_index(0x8005);
            let beneficiary_account = mapped_account(beneficiary);
            let contributor_account = mapped_account(contributor);
            let new_hotkey = AccountId::from([0x33; 32]);
            let lease_id = pallet_subtensor::NextSubnetLeaseId::<Runtime>::get();

            fund_account(&beneficiary_account, ACCOUNT_BALANCE);
            fund_account(&contributor_account, ACCOUNT_BALANCE);
            let crowdloan_id = create_lease_crowdloan(beneficiary);
            contribute_and_finalize(
                crowdloan_id,
                beneficiary_account.clone(),
                contributor_account.clone(),
            );

            let lease = pallet_subtensor::SubnetLeases::<Runtime>::get(lease_id)
                .expect("lease should exist");
            pallet_subtensor::Owner::<Runtime>::insert(&new_hotkey, &beneficiary_account);
            System::set_block_number(LEASING_END_BLOCK.into());

            precompiles::<LeasingPrecompile<Runtime>>()
                .prepare_test(
                    beneficiary,
                    addr_from_index(LeasingPrecompile::<Runtime>::INDEX),
                    encode_with_selector(
                        selector_u32("terminateLease(uint32,bytes32)"),
                        (lease_id, H256::from_slice(new_hotkey.as_slice())),
                    ),
                )
                .execute_returns(());

            assert!(pallet_subtensor::SubnetLeases::<Runtime>::get(lease_id).is_none());
            assert!(!pallet_subtensor::SubnetLeaseShares::<Runtime>::contains_prefix(lease_id));
            assert_eq!(
                pallet_subtensor::SubnetOwner::<Runtime>::get(lease.netuid),
                beneficiary_account,
            );
            assert_eq!(
                pallet_subtensor::SubnetOwnerHotkey::<Runtime>::get(lease.netuid),
                new_hotkey,
            );
        });
    }
}
