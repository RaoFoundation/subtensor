use alloc::string::String;
use core::marker::PhantomData;

use fp_evm::{ExitError, PrecompileFailure};
use frame_support::dispatch::{DispatchInfo, GetDispatchInfo, PostDispatchInfo};
use frame_support::traits::IsSubType;
use frame_system::RawOrigin;
use pallet_evm::AddressMapping;
use pallet_evm::PrecompileHandle;
use pallet_subtensor_proxy as pallet_proxy;
use precompile_utils::prelude::Address;
use precompile_utils::{EvmResult, solidity::Codec};
use sp_core::{ByteArray, H256};
use sp_runtime::traits::{AsSystemOriginSigner, Dispatchable, UniqueSaturatedInto};

use crate::{PrecompileExt, PrecompileHandleExt};

pub struct CrowdloanPrecompile<R>(PhantomData<R>);

impl<R> PrecompileExt<R::AccountId> for CrowdloanPrecompile<R>
where
    R: frame_system::Config
        + pallet_balances::Config
        + pallet_crowdloan::Config
        + pallet_evm::Config
        + pallet_proxy::Config
        + pallet_subtensor::Config
        + pallet_shield::Config
        + pallet_subtensor_proxy::Config
        + Send
        + Sync
        + scale_info::TypeInfo,
    R::AccountId: From<[u8; 32]> + ByteArray,
    <R as frame_system::Config>::RuntimeOrigin: AsSystemOriginSigner<R::AccountId> + Clone,
    <R as frame_system::Config>::RuntimeCall: From<pallet_crowdloan::Call<R>>
        + GetDispatchInfo
        + Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>
        + IsSubType<pallet_balances::Call<R>>
        + IsSubType<pallet_subtensor::Call<R>>
        + IsSubType<pallet_shield::Call<R>>
        + IsSubType<pallet_subtensor_proxy::Call<R>>,
    <R as pallet_evm::Config>::AddressMapping: AddressMapping<R::AccountId>,
{
    const INDEX: u64 = 2057;
}

#[precompile_utils::precompile]
impl<R> CrowdloanPrecompile<R>
where
    R: frame_system::Config
        + pallet_balances::Config
        + pallet_crowdloan::Config
        + pallet_evm::Config
        + pallet_proxy::Config
        + pallet_subtensor::Config
        + pallet_shield::Config
        + pallet_subtensor_proxy::Config
        + Send
        + Sync
        + scale_info::TypeInfo,
    R::AccountId: From<[u8; 32]> + ByteArray,
    <R as frame_system::Config>::RuntimeOrigin: AsSystemOriginSigner<R::AccountId> + Clone,
    <R as frame_system::Config>::RuntimeCall: From<pallet_crowdloan::Call<R>>
        + GetDispatchInfo
        + Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>
        + IsSubType<pallet_balances::Call<R>>
        + IsSubType<pallet_subtensor::Call<R>>
        + IsSubType<pallet_shield::Call<R>>
        + IsSubType<pallet_subtensor_proxy::Call<R>>,
    <R as frame_system::Config>::RuntimeCall: From<pallet_crowdloan::Call<R>>
        + GetDispatchInfo
        + Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>,
    <R as pallet_evm::Config>::AddressMapping: AddressMapping<R::AccountId>,
{
    #[precompile::public("getCrowdloan(uint32)")]
    #[precompile::view]
    fn get_crowdloan(
        handle: &mut impl PrecompileHandle,
        crowdloan_id: u32,
    ) -> EvmResult<CrowdloanInfo> {
        handle.record_db_reads::<R>(1)?;
        let crowdloan = pallet_crowdloan::Crowdloans::<R>::get(crowdloan_id).ok_or(
            PrecompileFailure::Error {
                exit_status: ExitError::Other("Crowdloan not found".into()),
            },
        )?;

        Ok(CrowdloanInfo {
            creator: H256::from_slice(crowdloan.creator.as_slice()),
            deposit: u64::from(crowdloan.deposit),
            min_contribution: u64::from(crowdloan.min_contribution),
            end: crowdloan.end.unique_saturated_into(),
            cap: u64::from(crowdloan.cap),
            funds_account: H256::from_slice(crowdloan.funds_account.as_slice()),
            raised: u64::from(crowdloan.raised),
            has_target_address: crowdloan.target_address.is_some(),
            target_address: crowdloan
                .target_address
                .map(|a| H256::from_slice(a.as_slice()))
                .unwrap_or_else(H256::zero),
            finalized: crowdloan.finalized,
            contributors_count: crowdloan.contributors_count,
        })
    }

    #[precompile::public("getContribution(uint32,bytes32)")]
    #[precompile::view]
    fn get_contribution(
        handle: &mut impl PrecompileHandle,
        crowdloan_id: u32,
        coldkey: H256,
    ) -> EvmResult<u64> {
        handle.record_db_reads::<R>(1)?;
        let coldkey = R::AccountId::from(coldkey.0);
        let contribution = pallet_crowdloan::Contributions::<R>::get(crowdloan_id, coldkey).ok_or(
            PrecompileFailure::Error {
                exit_status: ExitError::Other("Crowdloan or contribution not found".into()),
            },
        )?;

        Ok(u64::from(contribution))
    }

    #[precompile::public("create(uint64,uint64,uint64,uint32,address)")]
    #[precompile::payable]
    fn create(
        handle: &mut impl PrecompileHandle,
        deposit: u64,
        min_contribution: u64,
        cap: u64,
        end: u32,
        target_address: Address,
    ) -> EvmResult<()> {
        let who = handle.caller_account_id::<R>();
        let target_address = R::AddressMapping::into_account_id(target_address.0);
        let call = pallet_crowdloan::Call::<R>::create {
            deposit: deposit.into(),
            min_contribution: min_contribution.into(),
            cap: cap.into(),
            end: end.into(),
            call: None,
            target_address: Some(target_address),
        };

        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(who))
    }

    #[precompile::public("contribute(uint32,uint64)")]
    #[precompile::payable]
    fn contribute(
        handle: &mut impl PrecompileHandle,
        crowdloan_id: u32,
        amount: u64,
    ) -> EvmResult<()> {
        let account_id = handle.caller_account_id::<R>();
        let call = pallet_crowdloan::Call::<R>::contribute {
            crowdloan_id,
            amount: amount.into(),
        };

        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(account_id))
    }

    #[precompile::public("withdraw(uint32)")]
    #[precompile::payable]
    fn withdraw(handle: &mut impl PrecompileHandle, crowdloan_id: u32) -> EvmResult<()> {
        let account_id = handle.caller_account_id::<R>();
        let call = pallet_crowdloan::Call::<R>::withdraw { crowdloan_id };

        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(account_id))
    }

    #[precompile::public("finalize(uint32)")]
    #[precompile::payable]
    fn finalize(handle: &mut impl PrecompileHandle, crowdloan_id: u32) -> EvmResult<()> {
        let account_id = handle.caller_account_id::<R>();
        let call = pallet_crowdloan::Call::<R>::finalize { crowdloan_id };

        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(account_id))
    }

    #[precompile::public("refund(uint32)")]
    #[precompile::payable]
    fn refund(handle: &mut impl PrecompileHandle, crowdloan_id: u32) -> EvmResult<()> {
        let account_id = handle.caller_account_id::<R>();
        let call = pallet_crowdloan::Call::<R>::refund { crowdloan_id };

        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(account_id))
    }

    #[precompile::public("dissolve(uint32)")]
    #[precompile::payable]
    fn dissolve(handle: &mut impl PrecompileHandle, crowdloan_id: u32) -> EvmResult<()> {
        let account_id = handle.caller_account_id::<R>();
        let call = pallet_crowdloan::Call::<R>::dissolve { crowdloan_id };

        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(account_id))
    }

    #[precompile::public("updateMinContribution(uint32,uint64)")]
    #[precompile::payable]
    fn update_min_contribution(
        handle: &mut impl PrecompileHandle,
        crowdloan_id: u32,
        new_min_contribution: u64,
    ) -> EvmResult<()> {
        let account_id = handle.caller_account_id::<R>();
        let call = pallet_crowdloan::Call::<R>::update_min_contribution {
            crowdloan_id,
            new_min_contribution: new_min_contribution.into(),
        };

        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(account_id))
    }

    #[precompile::public("updateEnd(uint32,uint32)")]
    #[precompile::payable]
    fn update_end(
        handle: &mut impl PrecompileHandle,
        crowdloan_id: u32,
        new_end: u32,
    ) -> EvmResult<()> {
        let account_id = handle.caller_account_id::<R>();
        let call = pallet_crowdloan::Call::<R>::update_end {
            crowdloan_id,
            new_end: new_end.into(),
        };

        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(account_id))
    }

    #[precompile::public("updateCap(uint32,uint64)")]
    #[precompile::payable]
    fn update_cap(
        handle: &mut impl PrecompileHandle,
        crowdloan_id: u32,
        new_cap: u64,
    ) -> EvmResult<()> {
        let account_id = handle.caller_account_id::<R>();
        let call = pallet_crowdloan::Call::<R>::update_cap {
            crowdloan_id,
            new_cap: new_cap.into(),
        };

        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(account_id))
    }
}

#[derive(Codec)]
struct CrowdloanInfo {
    creator: H256,
    deposit: u64,
    min_contribution: u64,
    end: u32,
    cap: u64,
    funds_account: H256,
    raised: u64,
    has_target_address: bool,
    target_address: H256,
    finalized: bool,
    contributors_count: u32,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::arithmetic_side_effects)]

    use super::*;
    use crate::PrecompileExt;
    use crate::mock::{
        AccountId, Runtime, RuntimeOrigin, System, addr_from_index, fund_account, mapped_account,
        new_test_ext, precompiles, selector_u32,
    };
    use precompile_utils::solidity::{codec::Address, encode_return_value, encode_with_selector};
    use precompile_utils::testing::PrecompileTesterExt;
    use sp_core::H160;

    const CREATOR_DEPOSIT: u64 = 50;
    const MIN_CONTRIBUTION: u64 = 10;
    const CAP: u64 = 300;
    const END: u32 = 50;
    const ACCOUNT_BALANCE: u64 = 1_000;

    fn get_crowdloan(caller: H160, crowdloan_id: u32, expected: CrowdloanInfo) {
        let precompile_addr = addr_from_index(CrowdloanPrecompile::<Runtime>::INDEX);

        precompiles::<CrowdloanPrecompile<Runtime>>()
            .prepare_test(
                caller,
                precompile_addr,
                encode_with_selector(selector_u32("getCrowdloan(uint32)"), (crowdloan_id,)),
            )
            .with_static_call(true)
            .execute_returns_raw(encode_return_value(expected));
    }

    fn expected_crowdloan_info(crowdloan_id: u32) -> CrowdloanInfo {
        let crowdloan = pallet_crowdloan::Crowdloans::<Runtime>::get(crowdloan_id)
            .expect("crowdloan should exist");

        CrowdloanInfo {
            creator: H256::from_slice(crowdloan.creator.as_slice()),
            deposit: u64::from(crowdloan.deposit),
            min_contribution: u64::from(crowdloan.min_contribution),
            end: crowdloan.end as u32,
            cap: u64::from(crowdloan.cap),
            funds_account: H256::from_slice(crowdloan.funds_account.as_slice()),
            raised: u64::from(crowdloan.raised),
            has_target_address: crowdloan.target_address.is_some(),
            target_address: crowdloan
                .target_address
                .map(|account| H256::from_slice(account.as_slice()))
                .unwrap_or_else(H256::zero),
            finalized: crowdloan.finalized,
            contributors_count: crowdloan.contributors_count,
        }
    }

    fn create_crowdloan(caller: H160, target: H160) -> u32 {
        let crowdloan_id = pallet_crowdloan::NextCrowdloanId::<Runtime>::get();
        let precompile_addr = addr_from_index(CrowdloanPrecompile::<Runtime>::INDEX);

        precompiles::<CrowdloanPrecompile<Runtime>>()
            .prepare_test(
                caller,
                precompile_addr,
                encode_with_selector(
                    selector_u32("create(uint64,uint64,uint64,uint32,address)"),
                    (CREATOR_DEPOSIT, MIN_CONTRIBUTION, CAP, END, Address(target)),
                ),
            )
            .execute_returns(());
        crowdloan_id
    }

    fn contribute(caller: H160, crowdloan_id: u32, amount: u64) {
        let precompile_addr = addr_from_index(CrowdloanPrecompile::<Runtime>::INDEX);

        precompiles::<CrowdloanPrecompile<Runtime>>()
            .prepare_test(
                caller,
                precompile_addr,
                encode_with_selector(
                    selector_u32("contribute(uint32,uint64)"),
                    (crowdloan_id, amount),
                ),
            )
            .execute_returns(());
    }

    fn withdraw(caller: H160, crowdloan_id: u32) {
        let precompile_addr = addr_from_index(CrowdloanPrecompile::<Runtime>::INDEX);

        precompiles::<CrowdloanPrecompile<Runtime>>()
            .prepare_test(
                caller,
                precompile_addr,
                encode_with_selector(selector_u32("withdraw(uint32)"), (crowdloan_id,)),
            )
            .execute_returns(());
    }

    fn finalize(caller: H160, crowdloan_id: u32) {
        let precompile_addr = addr_from_index(CrowdloanPrecompile::<Runtime>::INDEX);

        precompiles::<CrowdloanPrecompile<Runtime>>()
            .prepare_test(
                caller,
                precompile_addr,
                encode_with_selector(selector_u32("finalize(uint32)"), (crowdloan_id,)),
            )
            .execute_returns(());
    }

    #[test]
    fn crowdloan_precompile_reads_existing_pallet_crowdloan() {
        new_test_ext().execute_with(|| {
            let creator = AccountId::from([0x11; 32]);
            let target = AccountId::from([0x12; 32]);
            let caller = addr_from_index(0x7001);
            let crowdloan_id = pallet_crowdloan::NextCrowdloanId::<Runtime>::get();

            fund_account(&creator, ACCOUNT_BALANCE);
            pallet_crowdloan::Pallet::<Runtime>::create(
                RuntimeOrigin::signed(creator),
                CREATOR_DEPOSIT.into(),
                MIN_CONTRIBUTION.into(),
                CAP.into(),
                END.into(),
                None,
                Some(target),
            )
            .expect("direct crowdloan create should work");

            get_crowdloan(caller, crowdloan_id, expected_crowdloan_info(crowdloan_id));
        });
    }

    #[test]
    fn crowdloan_precompile_creates_and_reads_crowdloan() {
        new_test_ext().execute_with(|| {
            let creator = addr_from_index(0x7002);
            let target = addr_from_index(0x7003);
            let creator_account = mapped_account(creator);
            let target_account = mapped_account(target);

            fund_account(&creator_account, ACCOUNT_BALANCE);

            let crowdloan_id = create_crowdloan(creator, target);
            let crowdloan = pallet_crowdloan::Crowdloans::<Runtime>::get(crowdloan_id)
                .expect("crowdloan should exist");

            assert_eq!(crowdloan.creator, creator_account);
            assert_eq!(u64::from(crowdloan.deposit), CREATOR_DEPOSIT);
            assert_eq!(u64::from(crowdloan.min_contribution), MIN_CONTRIBUTION);
            assert_eq!(u64::from(crowdloan.cap), CAP);
            assert_eq!(crowdloan.end, END as u64);
            assert_eq!(u64::from(crowdloan.raised), CREATOR_DEPOSIT);
            assert_eq!(crowdloan.target_address, Some(target_account));
            assert!(!crowdloan.finalized);
            assert_eq!(crowdloan.contributors_count, 1);
            get_crowdloan(creator, crowdloan_id, expected_crowdloan_info(crowdloan_id));
        });
    }

    #[test]
    fn crowdloan_precompile_contributes_and_withdraws() {
        new_test_ext().execute_with(|| {
            let creator = addr_from_index(0x7004);
            let contributor = addr_from_index(0x7005);
            let target = addr_from_index(0x7006);
            let creator_account = mapped_account(creator);
            let contributor_account = mapped_account(contributor);
            let contribution = 30_u64;

            fund_account(&creator_account, ACCOUNT_BALANCE);
            fund_account(&contributor_account, ACCOUNT_BALANCE);

            let crowdloan_id = create_crowdloan(creator, target);
            contribute(contributor, crowdloan_id, contribution);

            let crowdloan = pallet_crowdloan::Crowdloans::<Runtime>::get(crowdloan_id)
                .expect("crowdloan should exist");
            assert_eq!(u64::from(crowdloan.raised), CREATOR_DEPOSIT + contribution);
            assert_eq!(crowdloan.contributors_count, 2);
            assert_eq!(
                pallet_crowdloan::Contributions::<Runtime>::get(
                    crowdloan_id,
                    &contributor_account,
                ),
                Some(contribution.into()),
            );
            get_crowdloan(creator, crowdloan_id, expected_crowdloan_info(crowdloan_id));

            withdraw(contributor, crowdloan_id);

            let crowdloan = pallet_crowdloan::Crowdloans::<Runtime>::get(crowdloan_id)
                .expect("crowdloan should exist");
            assert_eq!(u64::from(crowdloan.raised), CREATOR_DEPOSIT);
            assert_eq!(crowdloan.contributors_count, 1);
            assert_eq!(
                pallet_crowdloan::Contributions::<Runtime>::get(
                    crowdloan_id,
                    &contributor_account,
                ),
                None,
            );
            get_crowdloan(creator, crowdloan_id, expected_crowdloan_info(crowdloan_id));
        });
    }

    #[test]
    fn crowdloan_precompile_contributes_and_withdraws_from_pallet_crowdloan() {
        new_test_ext().execute_with(|| {
            let creator = AccountId::from([0x22; 32]);
            let target = AccountId::from([0x23; 32]);
            let contributor = addr_from_index(0x7016);
            let contributor_account = mapped_account(contributor);
            let crowdloan_id = pallet_crowdloan::NextCrowdloanId::<Runtime>::get();
            let contribution = 30_u64;

            fund_account(&creator, ACCOUNT_BALANCE);
            fund_account(&contributor_account, ACCOUNT_BALANCE);
            pallet_crowdloan::Pallet::<Runtime>::create(
                RuntimeOrigin::signed(creator),
                CREATOR_DEPOSIT.into(),
                MIN_CONTRIBUTION.into(),
                CAP.into(),
                END.into(),
                None,
                Some(target),
            )
            .expect("direct crowdloan create should work");

            contribute(contributor, crowdloan_id, contribution);

            let crowdloan = pallet_crowdloan::Crowdloans::<Runtime>::get(crowdloan_id)
                .expect("crowdloan should exist");
            assert_eq!(u64::from(crowdloan.raised), CREATOR_DEPOSIT + contribution);
            assert_eq!(crowdloan.contributors_count, 2);
            get_crowdloan(contributor, crowdloan_id, expected_crowdloan_info(crowdloan_id));

            withdraw(contributor, crowdloan_id);

            let crowdloan = pallet_crowdloan::Crowdloans::<Runtime>::get(crowdloan_id)
                .expect("crowdloan should exist");
            assert_eq!(u64::from(crowdloan.raised), CREATOR_DEPOSIT);
            assert_eq!(crowdloan.contributors_count, 1);
            assert_eq!(
                pallet_crowdloan::Contributions::<Runtime>::get(
                    crowdloan_id,
                    &contributor_account,
                ),
                None,
            );
            get_crowdloan(contributor, crowdloan_id, expected_crowdloan_info(crowdloan_id));
        });
    }

    #[test]
    fn crowdloan_precompile_finalizes_capped_crowdloan() {
        new_test_ext().execute_with(|| {
            let creator = addr_from_index(0x7007);
            let contributor = addr_from_index(0x7008);
            let target = addr_from_index(0x7009);
            let creator_account = mapped_account(creator);
            let contributor_account = mapped_account(contributor);
            let target_account = mapped_account(target);

            fund_account(&creator_account, ACCOUNT_BALANCE);
            fund_account(&contributor_account, ACCOUNT_BALANCE);

            let crowdloan_id = create_crowdloan(creator, target);
            contribute(contributor, crowdloan_id, CAP - CREATOR_DEPOSIT);
            System::set_block_number(END.into());
            finalize(creator, crowdloan_id);

            let crowdloan = pallet_crowdloan::Crowdloans::<Runtime>::get(crowdloan_id)
                .expect("crowdloan should exist");
            assert!(crowdloan.finalized);
            assert_eq!(
                pallet_balances::Pallet::<Runtime>::free_balance(&target_account),
                CAP.into(),
            );
            get_crowdloan(creator, crowdloan_id, expected_crowdloan_info(crowdloan_id));
        });
    }

    #[test]
    fn crowdloan_precompile_refunds_and_dissolves_crowdloan() {
        new_test_ext().execute_with(|| {
            let creator = addr_from_index(0x7010);
            let first = addr_from_index(0x7011);
            let second = addr_from_index(0x7012);
            let target = addr_from_index(0x7013);
            let creator_account = mapped_account(creator);
            let first_account = mapped_account(first);
            let second_account = mapped_account(second);
            let contribution = 30_u64;

            fund_account(&creator_account, ACCOUNT_BALANCE);
            fund_account(&first_account, ACCOUNT_BALANCE);
            fund_account(&second_account, ACCOUNT_BALANCE);

            let crowdloan_id = create_crowdloan(creator, target);
            contribute(first, crowdloan_id, contribution);
            contribute(second, crowdloan_id, contribution);
            System::set_block_number(END.into());
            let precompile_addr = addr_from_index(CrowdloanPrecompile::<Runtime>::INDEX);

            precompiles::<CrowdloanPrecompile<Runtime>>()
                .prepare_test(
                    creator,
                    precompile_addr,
                    encode_with_selector(selector_u32("refund(uint32)"), (crowdloan_id,)),
                )
                .execute_returns(());

            let crowdloan = pallet_crowdloan::Crowdloans::<Runtime>::get(crowdloan_id)
                .expect("crowdloan should exist");
            assert_eq!(u64::from(crowdloan.raised), CREATOR_DEPOSIT);
            assert_eq!(crowdloan.contributors_count, 1);
            get_crowdloan(creator, crowdloan_id, expected_crowdloan_info(crowdloan_id));

            precompiles::<CrowdloanPrecompile<Runtime>>()
                .prepare_test(
                    creator,
                    precompile_addr,
                    encode_with_selector(selector_u32("dissolve(uint32)"), (crowdloan_id,)),
                )
                .execute_returns(());

            assert!(pallet_crowdloan::Crowdloans::<Runtime>::get(crowdloan_id).is_none());
        });
    }

    #[test]
    fn crowdloan_precompile_updates_crowdloan_terms() {
        new_test_ext().execute_with(|| {
            let creator = addr_from_index(0x7014);
            let target = addr_from_index(0x7015);
            let creator_account = mapped_account(creator);
            let new_min_contribution = 20_u64;
            let new_end = 80_u32;
            let new_cap = 400_u64;

            fund_account(&creator_account, ACCOUNT_BALANCE);

            let crowdloan_id = create_crowdloan(creator, target);
            let precompiles = precompiles::<CrowdloanPrecompile<Runtime>>();
            let precompile_addr = addr_from_index(CrowdloanPrecompile::<Runtime>::INDEX);

            precompiles
                .prepare_test(
                    creator,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("updateMinContribution(uint32,uint64)"),
                        (crowdloan_id, new_min_contribution),
                    ),
                )
                .execute_returns(());
            assert_eq!(
                u64::from(
                    pallet_crowdloan::Crowdloans::<Runtime>::get(crowdloan_id)
                        .expect("crowdloan should exist")
                        .min_contribution,
                ),
                new_min_contribution,
            );

            precompiles
                .prepare_test(
                    creator,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("updateEnd(uint32,uint32)"),
                        (crowdloan_id, new_end),
                    ),
                )
                .execute_returns(());
            assert_eq!(
                pallet_crowdloan::Crowdloans::<Runtime>::get(crowdloan_id)
                    .expect("crowdloan should exist")
                    .end,
                new_end as u64,
            );

            precompiles
                .prepare_test(
                    creator,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("updateCap(uint32,uint64)"),
                        (crowdloan_id, new_cap),
                    ),
                )
                .execute_returns(());
            assert_eq!(
                u64::from(
                    pallet_crowdloan::Crowdloans::<Runtime>::get(crowdloan_id)
                        .expect("crowdloan should exist")
                        .cap,
                ),
                new_cap,
            );
            get_crowdloan(creator, crowdloan_id, expected_crowdloan_info(crowdloan_id));
        });
    }
}
