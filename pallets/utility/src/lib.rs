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

//! # Utility Pallet (`pallet-subtensor-utility`)
//!
//! Stateless helpers for **batch dispatch**, **derivative (pseudonym) dispatch**, and
//! **origin-switched dispatch**. This pallet does **not** re-authenticate; it reuses the caller's
//! origin filters (except where root bypasses them).
//!
//! Subtensor fork notes (search: `with_weight`, `Normal`):
//! - [`Call::with_weight`] always reports [`frame_support::dispatch::DispatchClass::Normal`]
//!   (upstream FRAME may use Operational for the same extrinsic).
//! - Derivative account IDs are derived via blake2 of `("modlpy/utilisuba", who, index)` — see
//!   [`Pallet::derivative_account_id`].
//!
//! - [`Config`]
//! - [`Call`]
//! - [`Event`]
//! - [`Error`]
//!
//! ## Overview
//!
//! - **Batch dispatch** (`batch`, `batch_all`, `force_batch`): run many calls under one signature.
//!   - `batch`: stop on first error (`BatchInterrupted`), prior calls stay applied.
//!   - `batch_all`: atomic — any error rolls the whole extrinsic back; nested `batch_all` is filtered.
//!   - `force_batch`: never interrupt; emits `ItemFailed` / `BatchCompletedWithErrors` as needed.
//! - **Pseudonymal dispatch** (`as_derivative`): signed origin executes as a derived account ID.
//!   Proxy filters treat the derivative as the original origin.
//! - **Origin switch** (`dispatch_as`, `dispatch_as_fallible`, `with_weight`, `if_else`): root (or
//!   filtered signed for `if_else`) helpers for privileged or fallback dispatch.
//!
//! Since proxy filters are respected in all dispatches of this pallet, it should never need to be
//! filtered by any proxy.
//!
//! ## Interface
//!
//! ### Dispatchable Functions
//!
//! | Call | Role |
//! |------|------|
//! | `batch` | Fail-fast multi-call |
//! | `batch_all` | Atomic multi-call |
//! | `force_batch` | Continue-on-error multi-call |
//! | `as_derivative` | Dispatch as indexed derivative account |
//! | `dispatch_as` | Root: dispatch as `PalletsOrigin` (errors become events) |
//! | `dispatch_as_fallible` | Root: same, but forwards inner error |
//! | `with_weight` | Root: dispatch with caller-supplied weight witness |
//! | `if_else` | Main call, else fallback |)

// Ensure we're `no_std` when compiling for Wasm.
#![cfg_attr(not(feature = "std"), no_std)]

mod benchmarking;
mod tests;
pub mod weights;

extern crate alloc;

use alloc::{boxed::Box, vec::Vec};
use codec::{Decode, Encode};
use frame_support::{
    dispatch::{GetDispatchInfo, PostDispatchInfo, extract_actual_weight},
    traits::{IsSubType, OriginTrait, UnfilteredDispatchable},
};
use sp_core::TypeId;
use sp_io::hashing::blake2_256;
use sp_runtime::{
    DispatchError,
    traits::{BadOrigin, Dispatchable, TrailingZeroInput},
};
pub use weights::WeightInfo;

use subtensor_macros::freeze_struct;

pub use pallet::*;

#[frame_support::pallet]
#[allow(clippy::expect_used)]
pub mod pallet {
    use super::*;
    use frame_support::{dispatch::DispatchClass, pallet_prelude::*};
    use frame_system::pallet_prelude::*;

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    /// Configuration trait for the utility pallet (batch / derivative / dispatch-as helpers).
    #[pallet::config]
    pub trait Config: frame_system::Config {
        /// Runtime call type that utility may nest and dispatch (must include this pallet's `Call`).
        type RuntimeCall: Parameter
            + Dispatchable<RuntimeOrigin = Self::RuntimeOrigin, PostInfo = PostDispatchInfo>
            + GetDispatchInfo
            + From<frame_system::Call<Self>>
            + UnfilteredDispatchable<RuntimeOrigin = Self::RuntimeOrigin>
            + IsSubType<Call<Self>>
            + IsType<<Self as frame_system::Config>::RuntimeCall>;

        /// Outer origin caller type used by [`Call::dispatch_as`] / [`Call::dispatch_as_fallible`].
        type PalletsOrigin: Parameter +
			Into<<Self as frame_system::Config>::RuntimeOrigin> +
			IsType<<<Self as frame_system::Config>::RuntimeOrigin as frame_support::traits::OriginTrait>::PalletsOrigin>;

        /// Weight information for extrinsics in this pallet.
        type WeightInfo: WeightInfo;
    }

    /// Events emitted by batch, dispatch-as, and if-else helpers.
    ///
    /// Variant **order is frozen** (SCALE / metadata); do not reorder.
    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event {
        /// `batch` stopped early: `index` is the first failing call; prior calls remain applied.
        BatchInterrupted { index: u32, error: DispatchError },
        /// All items in a `batch` / `batch_all` / `force_batch` succeeded.
        BatchCompleted,
        /// `force_batch` finished with at least one `ItemFailed`.
        BatchCompletedWithErrors,
        /// One nested call inside a batch succeeded.
        ItemCompleted,
        /// One nested call inside `force_batch` failed; batch continued.
        ItemFailed { error: DispatchError },
        /// Result of [`Call::dispatch_as`] / successful [`Call::dispatch_as_fallible`].
        DispatchedAs { result: DispatchResult },
        /// [`Call::if_else`] main path succeeded; fallback was not run.
        IfElseMainSuccess,
        /// [`Call::if_else`] main failed and fallback was dispatched (`main_error` preserved).
        IfElseFallbackCalled { main_error: DispatchError },
    }

    // Align the call size to 1KB. As we are currently compiling the runtime for native/wasm
    // the `size_of` of the `Call` can be different. To ensure that this don't leads to
    // mismatches between native/wasm or to different metadata for the same runtime, we
    // algin the call size. The value is chosen big enough to hopefully never reach it.
    const CALL_ALIGN: u32 = 1024;

    #[pallet::extra_constants]
    impl<T: Config> Pallet<T> {
        /// Max nested calls allowed in `batch` / `batch_all` / `force_batch` (allocation-safe).
        fn batched_calls_limit() -> u32 {
            let allocator_limit = sp_core::MAX_POSSIBLE_ALLOCATION;
            let call_size = (core::mem::size_of::<<T as Config>::RuntimeCall>() as u32)
                .div_ceil(CALL_ALIGN)
                .saturating_mul(CALL_ALIGN);
            // The margin to take into account vec doubling capacity.
            let margin_factor = 3;

            allocator_limit
                .checked_div(margin_factor)
                .map_or(0, |x| x.checked_div(call_size).unwrap_or(0))
        }
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        fn integrity_test() {
            // If you hit this error, you need to try to `Box` big dispatchable parameters.
            assert!(
                core::mem::size_of::<<T as Config>::RuntimeCall>() as u32 <= CALL_ALIGN,
                "Call enum size should be smaller than {CALL_ALIGN} bytes.",
            );
        }
    }

    /// Dispatch errors for the utility pallet.
    ///
    /// Variant **order is frozen** (SCALE / metadata); do not reorder.
    #[pallet::error]
    pub enum Error<T> {
        /// `calls.len()` exceeded [`Pallet::batched_calls_limit`].
        TooManyCalls,
        /// [`Pallet::derivative_account_id`] could not decode the blake2 entropy into `AccountId`.
        InvalidDerivedAccount,
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        #![deny(clippy::expect_used)]

        /// Fail-fast batch: dispatch `calls` from the same origin; stop on first error.
        ///
        /// May be called from any origin except `None`. Root bypasses origin / base call filters.
        ///
        /// Always returns `Ok`; inspect events for outcome:
        /// - [`Event::BatchCompleted`] — all items succeeded
        /// - [`Event::BatchInterrupted`] — first failure at `index` (prior items stay applied)
        ///
        /// Caps at `batched_calls_limit`. Weight is base + sum of actual inner weights.
        #[pallet::call_index(0)]
        #[pallet::weight({
			let (dispatch_weight, pays) = Pallet::<T>::weight_and_dispatch_class(calls);
			let dispatch_weight = dispatch_weight.saturating_add(T::WeightInfo::batch(calls.len() as u32));
			(dispatch_weight, DispatchClass::Normal, pays)
		})]
        pub fn batch(
            origin: OriginFor<T>,
            calls: Vec<<T as Config>::RuntimeCall>,
        ) -> DispatchResultWithPostInfo {
            // Do not allow the `None` origin.
            if ensure_none(origin.clone()).is_ok() {
                return Err(BadOrigin.into());
            }

            let is_root = ensure_root(origin.clone()).is_ok();
            let calls_len = calls.len();
            ensure!(
                calls_len <= Self::batched_calls_limit() as usize,
                Error::<T>::TooManyCalls
            );

            // Track the actual weight of each of the batch calls.
            let mut weight = Weight::zero();
            for (index, call) in calls.into_iter().enumerate() {
                let info = call.get_dispatch_info();
                // If origin is root, don't apply any dispatch filters; root can call anything.
                let result = if is_root {
                    call.dispatch_bypass_filter(origin.clone())
                } else {
                    call.dispatch(origin.clone())
                };
                // Add the weight of this call.
                weight = weight.saturating_add(extract_actual_weight(&result, &info));
                if let Err(e) = result {
                    Self::deposit_event(Event::BatchInterrupted {
                        index: index as u32,
                        error: e.error,
                    });
                    // Take the weight of this function itself into account.
                    let base_weight = T::WeightInfo::batch(index.saturating_add(1) as u32);
                    // Return the actual used weight + base_weight of this call.
                    return Ok(Some(base_weight.saturating_add(weight)).into());
                }
                Self::deposit_event(Event::ItemCompleted);
            }
            Self::deposit_event(Event::BatchCompleted);
            let base_weight = T::WeightInfo::batch(calls_len as u32);
            Ok(Some(base_weight.saturating_add(weight)).into())
        }

        /// Dispatch `call` as the signed origin's derivative account at `index`.
        ///
        /// Origin must be **Signed**. Origin filters are preserved on the derivative caller
        /// (proxy filtering treats derivative ≡ original). See [`Pallet::derivative_account_id`].
        ///
        /// NOTE: To bypass account-based filtering after `proxy`, prefer Multisig
        /// `as_multi_threshold_1` instead. Historically named `as_limited_sub` (pre v12).
        #[pallet::call_index(1)]
        #[pallet::weight({
			let dispatch_info = call.get_dispatch_info();
			(
				T::WeightInfo::as_derivative()
					// AccountData for inner call origin accountdata.
					.saturating_add(T::DbWeight::get().reads_writes(1, 1))
					.saturating_add(dispatch_info.call_weight),
				DispatchClass::Normal,
			)
		})]
        pub fn as_derivative(
            origin: OriginFor<T>,
            index: u16,
            call: Box<<T as Config>::RuntimeCall>,
        ) -> DispatchResultWithPostInfo {
            let mut origin = origin;
            let who = ensure_signed(origin.clone())?;
            let pseudonym = Self::derivative_account_id(who, index)?;
            origin.set_caller_from(frame_system::RawOrigin::Signed(pseudonym));
            let info = call.get_dispatch_info();
            let result = call.dispatch(origin);
            // Always take into account the base weight of this call.
            let mut weight = T::WeightInfo::as_derivative()
                .saturating_add(T::DbWeight::get().reads_writes(1, 1));
            // Add the real weight of the dispatch.
            weight = weight.saturating_add(extract_actual_weight(&result, &info));
            result
                .map_err(|mut err| {
                    err.post_info = Some(weight).into();
                    err
                })
                .map(|_| Some(weight).into())
        }

        /// Atomic batch: dispatch all `calls` or roll the extrinsic back on any failure.
        ///
        /// May be called from any origin except `None`. Root bypasses filters. Nested
        /// [`Call::batch_all`] is rejected via an added origin filter (anti-reentrancy).
        /// Caps at `batched_calls_limit`.
        #[pallet::call_index(2)]
        #[pallet::weight({
			let (dispatch_weight, pays) = Pallet::<T>::weight_and_dispatch_class(calls);
			let dispatch_weight = dispatch_weight.saturating_add(T::WeightInfo::batch_all(calls.len() as u32));
			(dispatch_weight, DispatchClass::Normal, pays)
		})]
        pub fn batch_all(
            origin: OriginFor<T>,
            calls: Vec<<T as Config>::RuntimeCall>,
        ) -> DispatchResultWithPostInfo {
            // Do not allow the `None` origin.
            if ensure_none(origin.clone()).is_ok() {
                return Err(BadOrigin.into());
            }

            let is_root = ensure_root(origin.clone()).is_ok();
            let calls_len = calls.len();
            ensure!(
                calls_len <= Self::batched_calls_limit() as usize,
                Error::<T>::TooManyCalls
            );

            // Track the actual weight of each of the batch calls.
            let mut weight = Weight::zero();
            for (index, call) in calls.into_iter().enumerate() {
                let info = call.get_dispatch_info();
                // If origin is root, bypass any dispatch filter; root can call anything.
                let result = if is_root {
                    call.dispatch_bypass_filter(origin.clone())
                } else {
                    let mut filtered_origin = origin.clone();
                    // Don't allow users to nest `batch_all` calls.
                    filtered_origin.add_filter(
                        move |c: &<T as frame_system::Config>::RuntimeCall| {
                            let c = <T as Config>::RuntimeCall::from_ref(c);
                            !matches!(c.is_sub_type(), Some(Call::batch_all { .. }))
                        },
                    );
                    call.dispatch(filtered_origin)
                };
                // Add the weight of this call.
                weight = weight.saturating_add(extract_actual_weight(&result, &info));
                result.map_err(|mut err| {
                    // Take the weight of this function itself into account.
                    let base_weight = T::WeightInfo::batch_all(index.saturating_add(1) as u32);
                    // Return the actual used weight + base_weight of this call.
                    err.post_info = Some(base_weight.saturating_add(weight)).into();
                    err
                })?;
                Self::deposit_event(Event::ItemCompleted);
            }
            Self::deposit_event(Event::BatchCompleted);
            let base_weight = T::WeightInfo::batch_all(calls_len as u32);
            Ok(Some(base_weight.saturating_add(weight)).into())
        }

        /// Root-only: dispatch `call` under `as_origin`, recording the result in [`Event::DispatchedAs`].
        ///
        /// Does **not** return the inner call's error (use [`Call::dispatch_as_fallible`] for that).
        /// Bypasses origin filters via `dispatch_bypass_filter`.
        #[pallet::call_index(3)]
        #[pallet::weight({
			let dispatch_info = call.get_dispatch_info();
			(
				T::WeightInfo::dispatch_as()
					.saturating_add(dispatch_info.call_weight),
				DispatchClass::Normal,
			)
		})]
        pub fn dispatch_as(
            origin: OriginFor<T>,
            as_origin: Box<T::PalletsOrigin>,
            call: Box<<T as Config>::RuntimeCall>,
        ) -> DispatchResult {
            ensure_root(origin)?;

            let res = call.dispatch_bypass_filter((*as_origin).into());

            Self::deposit_event(Event::DispatchedAs {
                result: res.map(|_| ()).map_err(|e| e.error),
            });
            Ok(())
        }

        /// Continue-on-error batch: run every call; never abort the outer extrinsic on item failure.
        ///
        /// May be called from any origin except `None`. Root bypasses filters.
        /// Emits [`Event::ItemFailed`] / [`Event::ItemCompleted`] per item, then
        /// [`Event::BatchCompleted`] or [`Event::BatchCompletedWithErrors`].
        #[pallet::call_index(4)]
        #[pallet::weight({
			let (dispatch_weight, pays) = Pallet::<T>::weight_and_dispatch_class(calls);
			let dispatch_weight = dispatch_weight.saturating_add(T::WeightInfo::force_batch(calls.len() as u32));
			(dispatch_weight, DispatchClass::Normal, pays)
		})]
        pub fn force_batch(
            origin: OriginFor<T>,
            calls: Vec<<T as Config>::RuntimeCall>,
        ) -> DispatchResultWithPostInfo {
            // Do not allow the `None` origin.
            if ensure_none(origin.clone()).is_ok() {
                return Err(BadOrigin.into());
            }

            let is_root = ensure_root(origin.clone()).is_ok();
            let calls_len = calls.len();
            ensure!(
                calls_len <= Self::batched_calls_limit() as usize,
                Error::<T>::TooManyCalls
            );

            // Track the actual weight of each of the batch calls.
            let mut weight = Weight::zero();
            // Track failed dispatch occur.
            let mut has_error: bool = false;
            for call in calls.into_iter() {
                let info = call.get_dispatch_info();
                // If origin is root, don't apply any dispatch filters; root can call anything.
                let result = if is_root {
                    call.dispatch_bypass_filter(origin.clone())
                } else {
                    call.dispatch(origin.clone())
                };
                // Add the weight of this call.
                weight = weight.saturating_add(extract_actual_weight(&result, &info));
                if let Err(e) = result {
                    has_error = true;
                    Self::deposit_event(Event::ItemFailed { error: e.error });
                } else {
                    Self::deposit_event(Event::ItemCompleted);
                }
            }
            if has_error {
                Self::deposit_event(Event::BatchCompletedWithErrors);
            } else {
                Self::deposit_event(Event::BatchCompleted);
            }
            let base_weight = T::WeightInfo::force_batch(calls_len as u32);
            Ok(Some(base_weight.saturating_add(weight)).into())
        }

        /// Root-only: dispatch `call` as root using the supplied `weight` witness (not re-checked).
        ///
        /// Subtensor: outer dispatch class is always **Normal** (see module docs). Inner call still
        /// runs with root via `dispatch_bypass_filter`.
        #[allow(unknown_lints, benchmarked_weight_not_plugged)]
        #[pallet::call_index(5)]
        #[pallet::weight((*weight, DispatchClass::Normal))]
        pub fn with_weight(
            origin: OriginFor<T>,
            call: Box<<T as Config>::RuntimeCall>,
            weight: Weight,
        ) -> DispatchResult {
            ensure_root(origin)?;
            let _ = weight; // Explicitly don't check the the weight witness.

            let res = call.dispatch_bypass_filter(frame_system::RawOrigin::Root.into());
            res.map(|_| ()).map_err(|e| e.error)
        }

        /// Try `main`; on failure dispatch `fallback` (weights of both attempts accumulate).
        ///
        /// May be called from any origin except `None`. Root bypasses filters for both legs.
        /// - Main success → [`Event::IfElseMainSuccess`], fallback skipped.
        /// - Fallback success after main error → [`Event::IfElseFallbackCalled`].
        /// - Fallback failure → extrinsic errors with fallback's error and combined weight.
        #[pallet::call_index(6)]
        #[pallet::weight({
			let main = main.get_dispatch_info();
			let fallback = fallback.get_dispatch_info();
			(
				T::WeightInfo::if_else()
					.saturating_add(main.call_weight)
					.saturating_add(fallback.call_weight),
				DispatchClass::Normal,
			)
		})]
        pub fn if_else(
            origin: OriginFor<T>,
            main: Box<<T as Config>::RuntimeCall>,
            fallback: Box<<T as Config>::RuntimeCall>,
        ) -> DispatchResultWithPostInfo {
            // Do not allow the `None` origin.
            if ensure_none(origin.clone()).is_ok() {
                return Err(BadOrigin.into());
            }

            let is_root = ensure_root(origin.clone()).is_ok();

            // Track the weights
            let mut weight = T::WeightInfo::if_else();

            let main_info = main.get_dispatch_info();

            // Execute the main call first
            let main_result = if is_root {
                main.dispatch_bypass_filter(origin.clone())
            } else {
                main.dispatch(origin.clone())
            };

            // Add weight of the main call
            weight = weight.saturating_add(extract_actual_weight(&main_result, &main_info));

            let Err(main_error) = main_result else {
                // If the main result is Ok, we skip the fallback logic entirely
                Self::deposit_event(Event::IfElseMainSuccess);
                return Ok(Some(weight).into());
            };

            // If the main call failed, execute the fallback call
            let fallback_info = fallback.get_dispatch_info();

            let fallback_result = if is_root {
                fallback.dispatch_bypass_filter(origin.clone())
            } else {
                fallback.dispatch(origin)
            };

            // Add weight of the fallback call
            weight = weight.saturating_add(extract_actual_weight(&fallback_result, &fallback_info));

            let Err(fallback_error) = fallback_result else {
                // Fallback succeeded.
                Self::deposit_event(Event::IfElseFallbackCalled {
                    main_error: main_error.error,
                });
                return Ok(Some(weight).into());
            };

            // Both calls have failed, return fallback error
            Err(sp_runtime::DispatchErrorWithPostInfo {
                error: fallback_error.error,
                post_info: Some(weight).into(),
            })
        }

        /// Root-only: like [`Call::dispatch_as`], but forwards the inner call's error to the caller.
        ///
        /// On success still deposits [`Event::DispatchedAs`] with `Ok(())`.
        #[pallet::call_index(7)]
        #[pallet::weight({
			let dispatch_info = call.get_dispatch_info();
			(
				T::WeightInfo::dispatch_as_fallible()
					.saturating_add(dispatch_info.call_weight),
				DispatchClass::Normal,
			)
		})]
        pub fn dispatch_as_fallible(
            origin: OriginFor<T>,
            as_origin: Box<T::PalletsOrigin>,
            call: Box<<T as Config>::RuntimeCall>,
        ) -> DispatchResult {
            ensure_root(origin)?;

            call.dispatch_bypass_filter((*as_origin).into())
                .map_err(|e| e.error)?;

            Self::deposit_event(Event::DispatchedAs { result: Ok(()) });

            Ok(())
        }
    }

    impl<T: Config> Pallet<T> {
        /// Sum inner `call_weight`s and OR their `Pays` flags for batch weight annotations.
        ///
        /// Outer dispatch class is chosen separately (always `Normal` for this pallet's batches).
        fn weight_and_dispatch_class(calls: &[<T as Config>::RuntimeCall]) -> (Weight, Pays) {
            let mut total_weight = Weight::zero();
            let mut pays = Pays::No;

            for di in calls.iter().map(|call| call.get_dispatch_info()) {
                total_weight = total_weight.saturating_add(di.call_weight);
                if di.pays_fee == Pays::Yes {
                    pays = Pays::Yes;
                }
            }

            (total_weight, pays)
        }
    }
}

/// Legacy `TypeId` wrapper (`b"suba"`); not used by [`Pallet::derivative_account_id`].
///
/// Derivative IDs use the blake2 entropic path with prefix `modlpy/utilisuba` instead. Kept so the
/// frozen layout / TYPE_ID remain searchable if a migration ever reintroduces pallet-id encoding.
#[allow(unused)]
#[freeze_struct("17a7798f791a1a47")]
#[derive(Clone, Copy, Eq, PartialEq, Encode, Decode)]
struct IndexedUtilityPalletId(u16);

impl TypeId for IndexedUtilityPalletId {
    const TYPE_ID: [u8; 4] = *b"suba";
}

impl<T: Config> Pallet<T> {
    /// Derive the signed pseudonym account for `(who, index)` used by [`Call::as_derivative`].
    ///
    /// Entropy: `blake2_256(encode("modlpy/utilisuba", who, index))`, then decode as `AccountId`
    /// via [`TrailingZeroInput`]. Returns [`Error::InvalidDerivedAccount`] if decode fails.
    pub fn derivative_account_id(
        who: T::AccountId,
        index: u16,
    ) -> Result<T::AccountId, DispatchError> {
        let entropy = (b"modlpy/utilisuba", who, index).using_encoded(blake2_256);
        T::AccountId::decode(&mut TrailingZeroInput::new(entropy.as_ref()))
            .map_err(|_| Error::<T>::InvalidDerivedAccount.into())
    }
}
