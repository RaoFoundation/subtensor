//! Additional costs of a stateful address mapping. Stateless mappings pay zero.
//!
//! These are storage-operation counts, priced with the runtime's existing DB
//! weights, not substitute benchmark measurements for hashed registration.
//! They consume Frontier's external resource budget without changing Ethereum's
//! intrinsic gas, opcode costs or gas forwarded to a child call.

use crate::{AddressMapping, Config, EvmConfig, GasWeightMapping};
use frame_support::{traits::Get, weights::Weight};

pub fn weight<T: Config>(reads: u64) -> Weight {
	T::AddressMapping::extra_read_weight().saturating_mul(reads)
}

/// Round up: converting a nonzero storage weight must never undercharge gas.
pub fn gas<T: Config>(weight: Weight) -> u64 {
	let gas = T::GasWeightMapping::weight_to_gas(weight);
	let rounded = if T::GasWeightMapping::gas_to_weight(gas, false).ref_time() < weight.ref_time() {
		gas.saturating_add(1)
	} else {
		gas
	};
	rounded.max(
		weight
			.proof_size()
			.saturating_mul(T::GasLimitPovSizeRatio::get()),
	)
}

pub fn transaction_weight<T: Config>(create: bool, authorizations: usize) -> Weight {
	// Six fixed reads: sender admission, pre-dispatch guard, account validation,
	// dispatch guard, fee withdrawal and refund. Native paths use at most this.
	// CALL adds its nonce update. CREATE adds two nonce updates and five basic
	// reads (address derivation, including tracing, caller balance, collision).
	// Both prepay two transfer mappings, before entering the execution frame.
	// An authorization adds two policy reads, two basic reads, and its nonce
	// and code updates. Prepay these so exhaustion cannot split the two writes.
	weight::<T>(
		(if create { 15_u64 } else { 9_u64 })
			.saturating_add((authorizations as u64).saturating_mul(6)),
	)
}

/// Stateful mappings need a resource meter even when PoV metering is disabled.
/// Admission and execution must use the same fallback so unaffordable mapping
/// work is rejected before authorization recovery or any execution writes.
/// An explicit budget may tighten the gas-derived bound, but must not permit
/// effective gas (and therefore the fee) to exceed the transaction's gas limit.
pub fn external_weight_limit<T: Config>(
	gas_limit: u64,
	weight_limit: Option<Weight>,
) -> Option<Weight> {
	if T::AddressMapping::extra_read_weight() == Weight::zero() {
		return weight_limit;
	}
	let gas_weight = T::GasWeightMapping::gas_to_weight(gas_limit, false);
	Some(match weight_limit {
		Some(limit) => Weight::from_parts(
			limit.ref_time().min(gas_weight.ref_time()),
			if T::GasLimitPovSizeRatio::get() > 0 {
				limit.proof_size().min(gas_weight.proof_size())
			} else {
				// With no PoV gas ratio an explicit proof budget is independent
				// of gas, and must not be replaced by a zero derived proof bound.
				limit.proof_size()
			},
		),
		None => gas_weight,
	})
}

/// Cheap admission before signature recovery, including callers skipping validate.
/// Calldata and access-list intrinsic costs are checked by Frontier afterwards.
pub fn check_budget<T: Config>(
	config: &EvmConfig,
	create: bool,
	authorizations: usize,
	gas_limit: u64,
	weight_limit: Option<Weight>,
	proof_size_base_cost: Option<u64>,
) -> Result<(), fp_evm::TransactionValidationError> {
	use fp_evm::TransactionValidationError;
	if authorizations > 255 {
		return Err(TransactionValidationError::AuthorizationListTooLarge);
	}
	let minimum = (if create {
		config.gas_transaction_create
	} else {
		config.gas_transaction_call
	})
	.saturating_add(
		config
			.gas_per_empty_account_cost
			.saturating_mul(authorizations as u64),
	);
	if gas_limit < minimum {
		return Err(TransactionValidationError::GasLimitTooLow);
	}
	if let Some(limit) = external_weight_limit::<T>(gas_limit, weight_limit) {
		let cost = transaction_weight::<T>(create, authorizations);
		if limit.ref_time() < cost.ref_time()
			|| proof_size_base_cost
				.is_some_and(|base| limit.proof_size() < base.saturating_add(cost.proof_size()))
		{
			return Err(TransactionValidationError::GasLimitTooLow);
		}
	}
	Ok(())
}

/// Charge a precompile's mapping lookup before the infallible mapper reads storage.
/// External costs survive failed child calls and are included in effective gas
/// settlement; they must not reduce the child's standard EVM gas allowance.
pub fn record_precompile_mapping_cost<T: Config>(
	handle: &mut (impl crate::PrecompileHandle + ?Sized),
) -> Result<(), crate::ExitError> {
	let cost = weight::<T>(1);
	handle.record_external_cost(Some(cost.ref_time()), Some(cost.proof_size()), None)?;
	Ok(())
}
