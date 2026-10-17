use sp_runtime::TransactionOutcome;
use sp_blockchain::ApplyExtrinsicFailed;
/// Only a pool transaction's validity failure permits it to be skipped. An API
/// failure (including local execution backpressure) says nothing about validity
/// and must not produce a successful, incomplete pending-state response.
fn pending_extrinsic_outcome(
	result: Result<sp_runtime::ApplyExtrinsicResult, sp_api::ApiError>,
	is_inherent: bool,
) -> TransactionOutcome<Result<(), Error>> {
	match result {
		Ok(Ok(Err(error))) if is_inherent => {
			TransactionOutcome::Rollback(Err(Error::InherentDispatch(error)))
		}
		// Included user transactions consume their nonce and fee even when their
		// inner call fails. Keep those effects in the pending overlay.
		Ok(Ok(_)) => TransactionOutcome::Commit(Ok(())),
		Ok(Err(validity)) if is_inherent => {
			TransactionOutcome::Rollback(Err(ApplyExtrinsicFailed::Validity(validity).into()))
		}
		Ok(Err(_)) => TransactionOutcome::Rollback(Ok(())),
		Err(error) => TransactionOutcome::Rollback(Err(Error::from(error))),
	}
}

/// The generated error type for creating pending runtime api.
#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
	#[error("Failed to call runtime API, {0}")]
	CallApi(#[from] sp_api::ApiError),
	#[error("Failed to create pending inherent data, {0}")]
	PendingInherentData(#[from] sp_inherents::Error),
	#[error("Failed to create pending inherent data provider, {0}")]
	PendingCreateInherentDataProvider(#[from] Box<dyn std::error::Error + Send + Sync>),
	#[error(transparent)]
	Backend(#[from] sp_blockchain::Error),
	#[error(transparent)]
	ApplyExtrinsicFailed(#[from] ApplyExtrinsicFailed),
	#[error("Pending inherent dispatch failed: {0:?}")]
	InherentDispatch(sp_runtime::DispatchError),
}

#[cfg(test)]
mod tests {
	use super::*;
	use sp_runtime::{transaction_validity::InvalidTransaction, DispatchError};

	#[test]
	fn execution_capacity_and_runtime_failures_abort_pending_reconstruction() {
		for is_inherent in [false, true] {
			for message in [
				fp_rpc::LOCAL_EXECUTION_CAPACITY_ERROR,
				"runtime execution failed",
			] {
				let error = sp_api::ApiError::Application(std::io::Error::other(message).into());
				assert!(matches!(
					pending_extrinsic_outcome(Err(error), is_inherent),
					TransactionOutcome::Rollback(Err(Error::CallApi(_)))
				));
			}
		}
	}

	#[test]
	fn invalid_pool_transactions_are_skipped_but_invalid_inherents_abort() {
		assert!(matches!(
			pending_extrinsic_outcome(Ok(Err(InvalidTransaction::Stale.into())), false),
			TransactionOutcome::Rollback(Ok(()))
		));
		assert!(matches!(
			pending_extrinsic_outcome(Ok(Err(InvalidTransaction::BadMandatory.into())), true),
			TransactionOutcome::Rollback(Err(Error::ApplyExtrinsicFailed(_)))
		));
	}

	#[test]
	fn included_user_dispatch_errors_keep_fees_and_nonce_but_inherent_errors_abort() {
		assert!(matches!(
			pending_extrinsic_outcome(Ok(Ok(Err(DispatchError::BadOrigin))), false),
			TransactionOutcome::Commit(Ok(()))
		));
		assert!(matches!(
			pending_extrinsic_outcome(Ok(Ok(Err(DispatchError::BadOrigin))), true),
			TransactionOutcome::Rollback(Err(Error::InherentDispatch(DispatchError::BadOrigin)))
		));
		for is_inherent in [false, true] {
			assert!(matches!(
				pending_extrinsic_outcome(Ok(Ok(Ok(()))), is_inherent),
				TransactionOutcome::Commit(Ok(()))
			));
		}
	}
}
