// This file is part of Substrate.

// Copyright (C) Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

// Benchmarks for Utility Pallet

#![cfg(feature = "runtime-benchmarks")]

use alloc::vec;
use frame_benchmarking::{benchmarking::add_to_whitelist, v2::*};
use frame_system::RawOrigin;

use crate::*;

const SEED: u32 = 0;

fn assert_last_event<T: frame_system::pallet::Config>(
    generic_event: <T as frame_system::pallet::Config>::RuntimeEvent,
) {
    frame_system::Pallet::<T>::assert_last_event(generic_event.into());
}

fn stable_batch_calls<T: Config>(count: u32) -> alloc::vec::Vec<T::RuntimeCall> {
    vec![Call::<T>::benchmark_noop {}.into(); count as usize]
}

#[benchmarks]
mod benchmark {
    use super::*;

    #[benchmark]
    fn batch(c: Linear<0, 1000>) {
        let calls = vec![frame_system::Call::remark { remark: vec![] }.into(); c as usize];
        let caller = whitelisted_caller();

        #[extrinsic_call]
        _(RawOrigin::Signed(caller), calls);

        assert_last_event::<T>(Event::BatchCompleted.into());
    }

    #[benchmark]
    fn as_derivative() {
        let caller = account("caller", SEED, SEED);
        let call = Box::new(frame_system::Call::remark { remark: vec![] }.into());
        // Whitelist caller account from further DB operations.
        let caller_key = frame_system::Account::<T>::hashed_key_for(&caller);
        add_to_whitelist(caller_key.into());

        #[extrinsic_call]
        _(RawOrigin::Signed(caller), SEED as u16, call);
    }
    #[benchmark]
    fn batch_all(c: Linear<1, 1000>) {
        let calls = stable_batch_calls::<T>(c);
        let caller = whitelisted_caller();

        frame_system::Pallet::<T>::reset_events();

        #[extrinsic_call]
        _(RawOrigin::Signed(caller), calls);

        assert_last_event::<T>(Event::BatchCompleted.into());
    }

    #[benchmark]
    fn dispatch_as() {
        let caller = account("caller", SEED, SEED);
        let call = Box::new(frame_system::Call::remark { remark: vec![] }.into());
        let origin = T::RuntimeOrigin::from(RawOrigin::Signed(caller));
        let pallets_origin = origin.caller().clone();
        let pallets_origin = T::PalletsOrigin::from(pallets_origin);

        #[extrinsic_call]
        _(RawOrigin::Root, Box::new(pallets_origin), call);
    }
    #[benchmark]
    fn force_batch(c: Linear<1, 1000>) {
        let calls = stable_batch_calls::<T>(c);
        let caller = whitelisted_caller();

        frame_system::Pallet::<T>::reset_events();

        #[extrinsic_call]
        _(RawOrigin::Signed(caller), calls);

        assert_last_event::<T>(Event::BatchCompleted.into());
    }

    #[benchmark]
    fn dispatch_as_fallible() {
        let caller = account("caller", SEED, SEED);
        let call = Box::new(frame_system::Call::remark { remark: vec![] }.into());
        let origin: T::RuntimeOrigin = RawOrigin::Signed(caller).into();
        let pallets_origin = origin.caller().clone();
        let pallets_origin = T::PalletsOrigin::from(pallets_origin);

        #[extrinsic_call]
        _(RawOrigin::Root, Box::new(pallets_origin), call);
    }

    #[benchmark]
    fn if_else() {
        // Failing main call.
        let main_call = Box::new(frame_system::Call::set_code { code: vec![1] }.into());
        let fallback_call = Box::new(frame_system::Call::remark { remark: vec![1] }.into());
        let caller = whitelisted_caller();

        #[extrinsic_call]
        _(RawOrigin::Signed(caller), main_call, fallback_call);
    }

    #[benchmark]
    fn with_weight() {
        // `with_weight` trusts the supplied weight witness and dispatches the
        // inner call as root. Use the largest supplied witness and a real inner
        // dispatch so this benchmark measures the root bypass dispatch path.
        let call = Box::new(
            frame_system::Call::remark {
                remark: vec![0u8; 1024],
            }
            .into(),
        );
        let weight = frame_support::weights::Weight::from_parts(u64::MAX, u64::MAX);

        #[extrinsic_call]
        _(RawOrigin::Root, call, weight);
    }

    impl_benchmark_test_suite! {
        Pallet,
        tests::new_test_ext(),
        tests::Test
    }
}
