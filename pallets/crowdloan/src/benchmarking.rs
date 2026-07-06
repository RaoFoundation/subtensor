//! Benchmarks for Crowdloan Pallet
#![cfg(feature = "runtime-benchmarks")]
#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used
)]
use crate::{BalanceOf, CrowdloanId, CrowdloanInfo, CurrencyOf, pallet::*};
use codec::Encode;
use frame_benchmarking::{account, v2::*};
use frame_support::traits::{Get, QueryPreimage, StorePreimage, fungible::*};
use frame_system::{RawOrigin, pallet_prelude::BlockNumberFor};
use subtensor_runtime_common::{TaoBalance, Token};

extern crate alloc;

const SEED: u32 = 0;
const INLINE_PREIMAGE_LIMIT: usize = 128;

use alloc::{boxed::Box, vec};

fn assert_last_event<T: frame_system::pallet::Config>(
    generic_event: <T as frame_system::pallet::Config>::RuntimeEvent,
) {
    let events = frame_system::Pallet::<T>::events();
    let system_event: <T as frame_system::pallet::Config>::RuntimeEvent = generic_event.into();
    // compare to the last event record
    let frame_system::EventRecord { event, .. } = &events[events.len() - 1];
    assert_eq!(event, &system_event);
}

#[benchmarks]
mod benchmarks {
    use super::*;

    #[benchmark]
    fn create() {
        let creator: T::AccountId = account::<T::AccountId>("creator", 0, SEED);
        let deposit = T::MinimumDeposit::get();
        let min_contribution = T::AbsoluteMinimumContribution::get();
        let cap = deposit + deposit;
        let now = frame_system::Pallet::<T>::block_number();
        let end = now + T::MaximumBlockDuration::get();
        let call: Box<<T as Config>::RuntimeCall> =
            Box::new(frame_system::Call::<T>::remark { remark: vec![] }.into());
        let _ = CurrencyOf::<T>::set_balance(&creator, deposit);

        #[extrinsic_call]
        _(
            RawOrigin::Signed(creator.clone()),
            deposit,
            min_contribution,
            cap,
            end,
            Some(call.clone()),
            None,
        );

        // ensure the crowdloan is stored correctly
        let crowdloan_id = 0;
        let funds_account = Pallet::<T>::funds_account(crowdloan_id);
        assert_eq!(
            Crowdloans::<T>::get(crowdloan_id),
            Some(CrowdloanInfo {
                creator: creator.clone(),
                deposit,
                min_contribution,
                cap,
                end,
                funds_account: funds_account.clone(),
                raised: deposit,
                target_address: None,
                call: Some(T::Preimages::bound(*call).unwrap()),
                finalized: false,
                contributors_count: 1,
            })
        );
        // ensure the creator has been deducted the deposit
        assert!(CurrencyOf::<T>::balance(&creator) == TaoBalance::ZERO);
        // ensure the initial deposit is stored correctly as contribution
        assert_eq!(
            Contributions::<T>::get(crowdloan_id, &creator),
            Some(deposit)
        );
        // ensure the raised amount is updated correctly
        assert!(Crowdloans::<T>::get(crowdloan_id).is_some_and(|c| c.raised == deposit));
        // ensure the crowdloan account has the deposit
        assert_eq!(CurrencyOf::<T>::balance(&funds_account), deposit);
        // ensure the event is emitted
        assert_last_event::<T>(
            Event::<T>::Created {
                crowdloan_id,
                creator,
                end,
                cap,
            }
            .into(),
        );
        // ensure next crowdloan id is incremented
        assert_eq!(NextCrowdloanId::<T>::get(), crowdloan_id + 1);
    }

    #[benchmark]
    fn contribute() {
        // create a crowdloan
        let creator: T::AccountId = account::<T::AccountId>("creator", 0, SEED);
        let deposit = T::MinimumDeposit::get();
        let min_contribution = T::AbsoluteMinimumContribution::get();
        let cap = deposit + deposit;
        let now = frame_system::Pallet::<T>::block_number();
        let end = now + T::MaximumBlockDuration::get();
        let call: Box<<T as Config>::RuntimeCall> =
            Box::new(frame_system::Call::<T>::remark { remark: vec![] }.into());
        let _ = CurrencyOf::<T>::set_balance(&creator, deposit);
        let _ = Pallet::<T>::create(
            RawOrigin::Signed(creator.clone()).into(),
            deposit,
            min_contribution,
            cap,
            end,
            Some(call),
            None,
        );

        // setup contributor
        let contributor: T::AccountId = account::<T::AccountId>("contributor", 0, SEED);
        let amount: BalanceOf<T> = min_contribution;
        let crowdloan_id: CrowdloanId = 0;
        let _ = CurrencyOf::<T>::set_balance(&contributor, amount);

        #[extrinsic_call]
        _(RawOrigin::Signed(contributor.clone()), crowdloan_id, amount);

        // ensure the contribution is stored correctly
        assert_eq!(
            Contributions::<T>::get(crowdloan_id, &contributor),
            Some(amount)
        );
        // ensure the contributor has been deducted the amount
        assert!(CurrencyOf::<T>::balance(&contributor) == TaoBalance::ZERO);
        // ensure the crowdloan raised amount is updated correctly
        assert!(Crowdloans::<T>::get(crowdloan_id).is_some_and(|c| c.raised == deposit + amount));
        // ensure the contribution is present in the crowdloan account
        assert_eq!(
            CurrencyOf::<T>::balance(&Pallet::<T>::funds_account(crowdloan_id)),
            deposit + amount
        );
        // ensure the event is emitted
        assert_last_event::<T>(
            Event::<T>::Contributed {
                contributor,
                crowdloan_id,
                amount,
            }
            .into(),
        );
    }

    #[benchmark]
    fn withdraw() {
        // create a crowdloan
        let creator: T::AccountId = account::<T::AccountId>("creator", 0, SEED);
        let deposit = T::MinimumDeposit::get();
        let min_contribution = T::AbsoluteMinimumContribution::get();
        let cap = deposit + deposit;
        let now = frame_system::Pallet::<T>::block_number();
        let end = now + T::MaximumBlockDuration::get();
        let call: Box<<T as Config>::RuntimeCall> =
            Box::new(frame_system::Call::<T>::remark { remark: vec![] }.into());
        let _ = CurrencyOf::<T>::set_balance(&creator, deposit);
        let _ = Pallet::<T>::create(
            RawOrigin::Signed(creator.clone()).into(),
            deposit,
            min_contribution,
            cap,
            end,
            Some(call),
            None,
        );

        // create contribution
        let contributor: T::AccountId = account::<T::AccountId>("contributor", 0, SEED);
        let amount: BalanceOf<T> = min_contribution;
        let crowdloan_id: CrowdloanId = 0;
        let _ = CurrencyOf::<T>::set_balance(&contributor, amount);
        let _ = Pallet::<T>::contribute(
            RawOrigin::Signed(contributor.clone()).into(),
            crowdloan_id,
            amount,
        );

        // run to the end of the contribution period
        frame_system::Pallet::<T>::set_block_number(end);

        #[extrinsic_call]
        _(RawOrigin::Signed(contributor.clone()), crowdloan_id);

        // ensure the creator contribution has been removed
        assert_eq!(Contributions::<T>::get(crowdloan_id, &contributor), None);
        // ensure the contributor has his contribution back in his balance
        assert_eq!(CurrencyOf::<T>::balance(&contributor), amount);
        // ensure the crowdloan account has been deducted the contribution
        assert_eq!(
            CurrencyOf::<T>::balance(&Pallet::<T>::funds_account(crowdloan_id)),
            deposit
        );
        // ensure the crowdloan raised amount is updated correctly
        assert!(Crowdloans::<T>::get(crowdloan_id).is_some_and(|c| c.raised == deposit));
        // ensure the event is emitted
        assert_last_event::<T>(
            Event::<T>::Withdrew {
                contributor,
                crowdloan_id,
                amount,
            }
            .into(),
        );
    }

    #[benchmark]
    fn finalize() {
        // create a crowdloan
        let creator: T::AccountId = account::<T::AccountId>("creator", 0, SEED);
        let deposit = T::MinimumDeposit::get();
        let min_contribution = T::AbsoluteMinimumContribution::get();
        let cap = deposit + deposit;
        let now = frame_system::Pallet::<T>::block_number();
        let end = now + T::MaximumBlockDuration::get();
        // Force the call through preimage storage instead of the inline bounded path.
        let call: Box<<T as Config>::RuntimeCall> = Box::new(
            frame_system::Call::<T>::remark {
                remark: vec![0; 256],
            }
            .into(),
        );
        assert!(call.encode().len() > INLINE_PREIMAGE_LIMIT);
        let _ = CurrencyOf::<T>::set_balance(&creator, deposit);
        let _ = Pallet::<T>::create(
            RawOrigin::Signed(creator.clone()).into(),
            deposit,
            min_contribution,
            cap,
            end,
            Some(call),
            None,
        );

        // create contribution fullfilling the cap
        let crowdloan_id: CrowdloanId = 0;
        let stored_call = Crowdloans::<T>::get(crowdloan_id)
            .and_then(|crowdloan| crowdloan.call)
            .expect("benchmark crowdloan should store a finalization call");
        assert!(stored_call.lookup_needed());
        assert!(T::Preimages::have(&stored_call));

        let contributor: T::AccountId = account::<T::AccountId>("contributor", 0, SEED);
        let amount: BalanceOf<T> = cap - deposit;
        let _ = CurrencyOf::<T>::set_balance(&contributor, amount);
        let _ = Pallet::<T>::contribute(
            RawOrigin::Signed(contributor.clone()).into(),
            crowdloan_id,
            amount,
        );

        // run to the end of the contribution period
        frame_system::Pallet::<T>::set_block_number(end);

        #[extrinsic_call]
        _(RawOrigin::Signed(creator.clone()), crowdloan_id);

        // ensure the crowdloan has been finalized
        assert!(Crowdloans::<T>::get(crowdloan_id).is_some_and(|c| c.finalized));
        // ensure the temporary finalization storage has been cleared
        assert!(CurrentCrowdloanId::<T>::get().is_none());
        // ensure the event is emitted
        assert_last_event::<T>(Event::<T>::Finalized { crowdloan_id }.into());
    }

    #[benchmark]
    fn refund(k: Linear<3, { T::RefundContributorsLimit::get() }>) {
        // create a crowdloan
        let creator: T::AccountId = account::<T::AccountId>("creator", 0, SEED);
        let deposit = T::MinimumDeposit::get();
        let min_contribution = T::AbsoluteMinimumContribution::get();
        let cap = deposit + deposit;
        let now = frame_system::Pallet::<T>::block_number();
        let end = now + T::MaximumBlockDuration::get();
        let call: Box<<T as Config>::RuntimeCall> =
            Box::new(frame_system::Call::<T>::remark { remark: vec![] }.into());
        let _ = CurrencyOf::<T>::set_balance(&creator, deposit);
        let _ = Pallet::<T>::create(
            RawOrigin::Signed(creator.clone()).into(),
            deposit,
            min_contribution,
            cap,
            end,
            Some(call),
            None,
        );

        let crowdloan_id: CrowdloanId = 0;
        let amount: BalanceOf<T> = min_contribution;
        // create the worst case count of contributors k to be refunded minus the creator
        // who is already a contributor
        let contributors = k - 1;
        for i in 0..contributors {
            let contributor: T::AccountId = account::<T::AccountId>("contributor", i, SEED);
            let _ = CurrencyOf::<T>::set_balance(&contributor, amount);
            let _ = Pallet::<T>::contribute(
                RawOrigin::Signed(contributor.clone()).into(),
                crowdloan_id,
                amount,
            );
        }

        // run to the end of the contribution period
        frame_system::Pallet::<T>::set_block_number(end);

        #[extrinsic_call]
        _(RawOrigin::Signed(creator.clone()), crowdloan_id);

        // ensure the creator has not been refunded and contribution is the actual initial deposit
        assert_eq!(CurrencyOf::<T>::balance(&creator), TaoBalance::ZERO);
        assert_eq!(
            Contributions::<T>::get(crowdloan_id, &creator),
            Some(deposit)
        );
        // ensure each contributor has been refunded and the contributions is removed
        for i in 0..contributors {
            let contributor: T::AccountId = account::<T::AccountId>("contributor", i, SEED);
            assert_eq!(CurrencyOf::<T>::balance(&contributor), amount);
            assert_eq!(Contributions::<T>::get(crowdloan_id, &contributor), None);
        }
        // ensure the crowdloan account has been deducted the contributions
        assert_eq!(
            CurrencyOf::<T>::balance(&Pallet::<T>::funds_account(crowdloan_id)),
            deposit
        );
        // ensure the raised amount is updated correctly
        assert!(Crowdloans::<T>::get(crowdloan_id).is_some_and(|c| c.raised == deposit));
        // ensure the event is emitted
        assert_last_event::<T>(Event::<T>::AllRefunded { crowdloan_id }.into());
    }

    #[benchmark]
    fn dissolve() {
        // create a crowdloan
        let creator: T::AccountId = account::<T::AccountId>("creator", 0, SEED);
        let deposit = T::MinimumDeposit::get();
        let min_contribution = T::AbsoluteMinimumContribution::get();
        let cap = deposit + deposit;
        let now = frame_system::Pallet::<T>::block_number();
        let end = now + T::MaximumBlockDuration::get();
        let call: Box<<T as Config>::RuntimeCall> =
            Box::new(frame_system::Call::<T>::remark { remark: vec![] }.into());
        let _ = CurrencyOf::<T>::set_balance(&creator, deposit);
        let _ = Pallet::<T>::create(
            RawOrigin::Signed(creator.clone()).into(),
            deposit,
            min_contribution,
            cap,
            end,
            Some(call),
            None,
        );

        // run to the end of the contribution period
        frame_system::Pallet::<T>::set_block_number(end);

        // refund the contributions
        let crowdloan_id: CrowdloanId = 0;
        let _ = Pallet::<T>::refund(RawOrigin::Signed(creator.clone()).into(), crowdloan_id);

        #[extrinsic_call]
        _(RawOrigin::Signed(creator.clone()), crowdloan_id);

        // ensure the crowdloan has been dissolved
        assert!(Crowdloans::<T>::get(crowdloan_id).is_none());
        // ensure the event is emitted
        assert_last_event::<T>(Event::<T>::Dissolved { crowdloan_id }.into());
    }

    #[benchmark]
    fn update_min_contribution() {
        // create a crowdloan
        let creator: T::AccountId = account::<T::AccountId>("creator", 0, SEED);
        let deposit = T::MinimumDeposit::get();
        let min_contribution = T::AbsoluteMinimumContribution::get();
        let cap = deposit + deposit;
        let end = frame_system::Pallet::<T>::block_number() + T::MaximumBlockDuration::get();
        let call: Box<<T as Config>::RuntimeCall> =
            Box::new(frame_system::Call::<T>::remark { remark: vec![] }.into());
        let _ = CurrencyOf::<T>::set_balance(&creator, deposit);
        let _ = Pallet::<T>::create(
            RawOrigin::Signed(creator.clone()).into(),
            deposit,
            min_contribution,
            cap,
            end,
            Some(call),
            None,
        );

        let crowdloan_id: CrowdloanId = 0;
        let new_min_contribution: BalanceOf<T> = min_contribution + min_contribution;

        #[extrinsic_call]
        _(
            RawOrigin::Signed(creator.clone()),
            crowdloan_id,
            new_min_contribution,
        );

        // ensure the min contribution is updated correctly
        assert!(
            Crowdloans::<T>::get(crowdloan_id)
                .is_some_and(|c| c.min_contribution == new_min_contribution)
        );
        // ensure the event is emitted
        assert_last_event::<T>(
            Event::<T>::MinContributionUpdated {
                crowdloan_id,
                new_min_contribution,
            }
            .into(),
        );
    }

    #[benchmark]
    fn update_end() {
        // create a crowdloan
        let creator: T::AccountId = account::<T::AccountId>("creator", 0, SEED);
        let deposit = T::MinimumDeposit::get();
        let min_contribution = T::AbsoluteMinimumContribution::get();
        let cap = deposit + deposit;
        let now = frame_system::Pallet::<T>::block_number();
        let end = now + T::MinimumBlockDuration::get();
        let call: Box<<T as Config>::RuntimeCall> =
            Box::new(frame_system::Call::<T>::remark { remark: vec![] }.into());
        let _ = CurrencyOf::<T>::set_balance(&creator, deposit);
        let _ = Pallet::<T>::create(
            RawOrigin::Signed(creator.clone()).into(),
            deposit,
            min_contribution,
            cap,
            end,
            Some(call),
            None,
        );

        let crowdloan_id: CrowdloanId = 0;
        let new_end: BlockNumberFor<T> = now + T::MaximumBlockDuration::get();

        #[extrinsic_call]
        _(RawOrigin::Signed(creator.clone()), crowdloan_id, new_end);

        // ensure the end is updated correctly
        assert!(Crowdloans::<T>::get(crowdloan_id).is_some_and(|c| c.end == new_end));
        // ensure the event is emitted
        assert_last_event::<T>(
            Event::<T>::EndUpdated {
                crowdloan_id,
                new_end,
            }
            .into(),
        );
    }

    #[benchmark]
    fn update_cap() {
        // create a crowdloan
        let creator: T::AccountId = account::<T::AccountId>("creator", 0, SEED);
        let deposit = T::MinimumDeposit::get();
        let min_contribution = T::AbsoluteMinimumContribution::get();
        let cap = deposit + deposit;
        let end = frame_system::Pallet::<T>::block_number() + T::MaximumBlockDuration::get();
        let call: Box<<T as Config>::RuntimeCall> =
            Box::new(frame_system::Call::<T>::remark { remark: vec![] }.into());
        let _ = CurrencyOf::<T>::set_balance(&creator, deposit);
        let _ = Pallet::<T>::create(
            RawOrigin::Signed(creator.clone()).into(),
            deposit,
            min_contribution,
            cap,
            end,
            Some(call),
            None,
        );

        let crowdloan_id: CrowdloanId = 0;
        let new_cap: BalanceOf<T> = cap + cap;

        #[extrinsic_call]
        _(RawOrigin::Signed(creator.clone()), crowdloan_id, new_cap);

        // ensure the cap is updated correctly
        assert!(Crowdloans::<T>::get(crowdloan_id).is_some_and(|c| c.cap == new_cap));
        // ensure the event is emitted
        assert_last_event::<T>(
            Event::<T>::CapUpdated {
                crowdloan_id,
                new_cap,
            }
            .into(),
        );
    }

    #[benchmark]
    fn set_max_contribution() {
        // create a crowdloan
        let creator: T::AccountId = account::<T::AccountId>("creator", 0, SEED);
        let deposit = T::MinimumDeposit::get();
        let min_contribution = T::AbsoluteMinimumContribution::get();
        let cap = deposit + deposit;
        let end = frame_system::Pallet::<T>::block_number() + T::MaximumBlockDuration::get();
        let call: Box<<T as Config>::RuntimeCall> =
            Box::new(frame_system::Call::<T>::remark { remark: vec![] }.into());
        let _ = CurrencyOf::<T>::set_balance(&creator, deposit);
        let _ = Pallet::<T>::create(
            RawOrigin::Signed(creator.clone()).into(),
            deposit,
            min_contribution,
            cap,
            end,
            Some(call),
            None,
        );

        let crowdloan_id: CrowdloanId = 0;
        let new_max_contribution: BalanceOf<T> = cap;

        #[extrinsic_call]
        _(
            RawOrigin::Signed(creator.clone()),
            crowdloan_id,
            Some(new_max_contribution),
        );

        // ensure the max contribution is updated correctly
        assert!(
            MaxContributions::<T>::get(crowdloan_id).is_some_and(|c| c == new_max_contribution)
        );
        // ensure the event is emitted
        assert_last_event::<T>(
            Event::<T>::MaxContributionUpdated {
                crowdloan_id,
                new_max_contribution: Some(new_max_contribution),
            }
            .into(),
        );
    }

    impl_benchmark_test_suite!(Pallet, crate::mock::new_test_ext(), crate::mock::Test);
}
