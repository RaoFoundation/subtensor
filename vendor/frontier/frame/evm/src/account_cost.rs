//! Additional costs of a stateful address mapping. Stateless mappings pay zero.
//!
//! These are storage-operation counts, priced with the runtime's existing DB
//! weights, not substitute benchmark measurements for hashed registration.

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
	// An authorization adds two policy reads, two basic reads, and its nonce
	// and code updates. Prepay these so exhaustion cannot split the two writes.
	weight::<T>(
		(if create { 13_u64 } else { 7_u64 })
			.saturating_add((authorizations as u64).saturating_mul(6)),
	)
}

pub fn config<T: Config>(config: &EvmConfig) -> EvmConfig {
	let mut config = config.clone();
	config.gas_transaction_call = config
		.gas_transaction_call
		.saturating_add(gas::<T>(weight::<T>(7)));
	config.gas_transaction_create = config
		.gas_transaction_create
		.saturating_add(gas::<T>(weight::<T>(13)));
	let authorization = gas::<T>(weight::<T>(6));
	config.gas_per_empty_account_cost = config
		.gas_per_empty_account_cost
		.saturating_add(authorization);
	// The policy/read surcharge must not be refunded for an existing authority.
	config.gas_auth_base_cost = config.gas_auth_base_cost.saturating_add(authorization);
	config
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
	let config = self::config::<T>(config);
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
	if let Some(limit) = weight_limit {
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

// AddressMapping is infallible, so collect precompile lookups here and charge
// them before the executor commits its precompile frame. No precompile's ABI,
// caller mapping or implementation needs to change.
environmental::environmental!(precompile_reads: u64);

pub fn record_precompile_mapping_read() {
	precompile_reads::with(|reads| *reads = reads.saturating_add(1));
}

pub struct MeteredPrecompiles<T, P>(P, core::marker::PhantomData<T>);

impl<T, P> MeteredPrecompiles<T, P> {
	pub fn new(inner: P) -> Self {
		Self(inner, core::marker::PhantomData)
	}
}

impl<T: Config, P: crate::PrecompileSet> crate::PrecompileSet for MeteredPrecompiles<T, P> {
	fn execute(
		&self,
		handle: &mut impl crate::PrecompileHandle,
	) -> Option<crate::PrecompileResult> {
		let mut reads = 0;
		let result = precompile_reads::using(&mut reads, || self.0.execute(handle));
		result.map(|result| {
			let cost = weight::<T>(reads);
			handle.record_cost(gas::<T>(cost))?;
			handle.record_external_cost(Some(cost.ref_time()), Some(cost.proof_size()), None)?;
			result
		})
	}

	fn is_precompile(&self, address: sp_core::H160, gas: u64) -> crate::IsPrecompileResult {
		self.0.is_precompile(address, gas)
	}
}
