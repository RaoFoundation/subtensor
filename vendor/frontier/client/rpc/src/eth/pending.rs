// This file is part of Frontier.

// Copyright (C) Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later WITH Classpath-exception-2.0

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

// Substrate
use sc_client_api::backend::{Backend, StorageProvider};
use sc_transaction_pool_api::{InPoolTransaction, TransactionPool};
use sp_api::{ApiExt, ApiRef, Core, ProvideRuntimeApi};
use sp_block_builder::BlockBuilder as BlockBuilderApi;
use sp_blockchain::{ApplyExtrinsicFailed, HeaderBackend};
use sp_inherents::{CreateInherentDataProviders, InherentData, InherentDataProvider};
use sp_runtime::{
	generic::Digest,
	traits::{Block as BlockT, Header as HeaderT, One},
	TransactionOutcome,
};

use crate::eth::Eth;
use fp_rpc::EthereumRuntimeRPCApi;

const LOG_TARGET: &str = "eth-pending";

/// Only a pool transaction's validity failure permits it to be skipped. An API
/// failure says nothing about validity and must not produce a successful,
/// incomplete pending-state response.
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

impl<B, C, P, CT, BE, CIDP, EC> Eth<B, C, P, CT, BE, CIDP, EC>
where
	B: BlockT,
	C: ProvideRuntimeApi<B>,
	C::Api: BlockBuilderApi<B>,
	C::Api: EthereumRuntimeRPCApi<B>,
	C: HeaderBackend<B> + StorageProvider<B, BE> + 'static,
	BE: Backend<B>,
	CIDP: CreateInherentDataProviders<B, ()> + Send + 'static,
	P: TransactionPool<Block = B, Hash = B::Hash> + 'static,
{
	/// Creates a pending runtime API.
	pub(crate) async fn pending_runtime_api(&self) -> Result<(B::Hash, ApiRef<'_, C::Api>), Error> {
		let api = self.client.runtime_api();
		// Pin the complete ready pool for this pending-state reconstruction.
		let extrinsics: Vec<_> = self
			.graph
			.ready()
			.map(|tx| tx.data().as_ref().clone())
			.collect();

		let info = self.client.info();
		let (best_number, best_hash) = (info.best_number, info.best_hash);

		let provider = self
			.pending_create_inherent_data_providers
			.create_inherent_data_providers(best_hash, ())
			.await?;
		let inherent_data = provider.create_inherent_data().await?;

		let digest = if let Some(digest_provider) = &self.pending_consensus_data_provider {
			if let Some(header) = self.client.header(best_hash)? {
				digest_provider.create_digest(&header, &inherent_data)?
			} else {
				Default::default()
			}
		} else {
			Default::default()
		};

		log::debug!(target: LOG_TARGET, "Pending runtime API: header digest = {digest:?}");

		let pending_header = <<B as BlockT>::Header as HeaderT>::new(
			best_number + One::one(),
			Default::default(),
			Default::default(),
			best_hash,
			digest,
		);

		// Roll back an unsuccessful initialization. Only a genuinely absent
		// legacy export permits fallback; execution failures do not.
		let initialized = api.execute_in_transaction(|api| {
			match api.initialize_pending_block(best_hash, &pending_header) {
				Ok(()) => TransactionOutcome::Commit(Ok(())),
				Err(error) => TransactionOutcome::Rollback(Err(error)),
			}
		});
		match initialized {
            Err(error) if error.to_string() == "Execution failed: Method not found: 'EthereumRuntimeRPCApi_initialize_pending_block'" => {
                api.execute_in_transaction(|api| {
                    match api.initialize_block(best_hash, &pending_header) {
                        Ok(_) => TransactionOutcome::Commit(Ok(())),
                        Err(error) => TransactionOutcome::Rollback(Err(error)),
                    }
                })?;
            }
            result => result?,
        }

		// Inherent construction must never commit storage effects.
		let inherents = api.execute_in_transaction(|api| {
			TransactionOutcome::Rollback(api.inherent_extrinsics(best_hash, inherent_data))
		})?;
		log::debug!(target: LOG_TARGET, "Pending runtime API: inherent len = {}, pool len = {}", inherents.len(), extrinsics.len());
		for (ext, is_inherent) in inherents
			.into_iter()
			.map(|ext| (ext, true))
			.chain(extrinsics.into_iter().map(|ext| (ext, false)))
		{
			api.execute_in_transaction(|api| {
				pending_extrinsic_outcome(api.apply_extrinsic(best_hash, ext), is_inherent)
			})?;
		}

		Ok((best_hash, api))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use sp_runtime::{transaction_validity::InvalidTransaction, DispatchError};

	#[test]
	fn runtime_failures_abort_pending_reconstruction() {
		for is_inherent in [false, true] {
			let error = sp_api::ApiError::Application(
				std::io::Error::other("runtime execution failed").into(),
			);
			assert!(matches!(
				pending_extrinsic_outcome(Err(error), is_inherent),
				TransactionOutcome::Rollback(Err(Error::CallApi(_)))
			));
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

/// Consensus data provider, pending api uses this trait object for authoring blocks valid for any runtime.
pub trait ConsensusDataProvider<B: BlockT>: Send + Sync {
	/// Attempt to create a consensus digest.
	fn create_digest(
		&self,
		parent: &B::Header,
		data: &InherentData,
	) -> Result<Digest, sp_inherents::Error>;
}

impl<B: BlockT> ConsensusDataProvider<B> for () {
	fn create_digest(
		&self,
		_: &B::Header,
		_: &InherentData,
	) -> Result<Digest, sp_inherents::Error> {
		Ok(Default::default())
	}
}
