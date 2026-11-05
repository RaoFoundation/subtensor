// This file is part of Frontier.

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

environmental::environmental!(STORAGE_OOG: bool);

use crate::{ExitError, ExitReason};
use sp_core::U256;

pub fn handle_storage_oog<R, F>(gas_limit: u64, f: F) -> (ExitReason, R, u64, U256)
where
	F: FnOnce() -> (ExitReason, R, u64, U256),
	R: Default,
{
	STORAGE_OOG::using_once(&mut false, || {
		let (reason, retv, used_gas, effective_gas) = f();

		STORAGE_OOG::with(|storage_oog| {
			if *storage_oog {
				(
					ExitReason::Error(ExitError::OutOfGas),
					Default::default(),
					used_gas,
					U256([gas_limit, 0, 0, 0]),
				)
			} else {
				(reason, retv, used_gas, effective_gas)
			}
		})
		// This should always return `Some`, but let's play it safe.
		.expect("STORAGE_OOG not defined")
	})
}

pub fn set_storage_oog() {
	STORAGE_OOG::with(|storage_oog| {
		*storage_oog = true;
	});
}

/// Whether the active execution has exhausted a transaction-wide resource limit.
/// Unlike a child's ordinary gas failure, this cannot be caught to commit state.
pub fn is_storage_oog() -> bool {
	STORAGE_OOG::with(|storage_oog| *storage_oog).unwrap_or(false)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{ExitSucceed, WeightInfo};
	use frame_support::weights::Weight;

	#[test]
	fn nested_execution_preserves_global_exhaustion_until_outer_scope_ends() {
		let success = ExitReason::Succeed(ExitSucceed::Stopped);
		assert!(!is_storage_oog());
		let (reason, value, _, gas) = handle_storage_oog(100, || {
			let (reason, value, _, gas) = handle_storage_oog(50, || {
				set_storage_oog();
				(success.clone(), 1u8, 10, U256::from(10))
			});
			assert_eq!(reason, ExitError::OutOfGas.into());
			assert_eq!(value, 0);
			assert_eq!(gas, U256::from(50));
			assert!(is_storage_oog());
			(success.clone(), 1u8, 20, U256::from(20))
		});
		assert_eq!(reason, ExitError::OutOfGas.into());
		assert_eq!(value, 0);
		assert_eq!(gas, U256::from(100));
		assert!(!is_storage_oog());
		assert_eq!(
			handle_storage_oog(100, || (success.clone(), 1u8, 20, U256::from(20))),
			(success, 1u8, 20, U256::from(20))
		);
	}

	#[test]
	fn weight_counter_overflow_cannot_be_caught_as_a_local_failure() {
		for proof_size in [false, true] {
			let (reason, value, _, gas) = handle_storage_oog(100, || {
				let mut info = WeightInfo::new_from_weight_limit(Some(Weight::MAX), Some(0))
					.unwrap()
					.unwrap();
				let error = if proof_size {
					info.try_record_proof_size_or_fail(u64::MAX).unwrap();
					info.try_record_proof_size_or_fail(1)
				} else {
					info.try_record_ref_time_or_fail(u64::MAX).unwrap();
					info.try_record_ref_time_or_fail(1)
				};
				assert_eq!(error, Err(ExitError::OutOfGas));
				assert!(is_storage_oog());
				// Simulate a parent catching the error and returning success.
				(
					ExitReason::Succeed(ExitSucceed::Stopped),
					1u8,
					10,
					U256::from(10),
				)
			});
			assert_eq!(reason, ExitError::OutOfGas.into());
			assert_eq!(value, 0);
			assert_eq!(gas, U256::from(100));
		}
	}
}
