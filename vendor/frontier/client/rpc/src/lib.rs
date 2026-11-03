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

#![allow(
	clippy::too_many_arguments,
	clippy::large_enum_variant,
	clippy::manual_range_contains,
	clippy::explicit_counter_loop,
	clippy::len_zero,
	clippy::new_without_default
)]
#![warn(unused_crate_dependencies)]

mod cache;
mod debug;
mod eth;
mod eth_pubsub;
#[doc(hidden)]
pub mod execution_admission;
mod net;
mod signer;
#[cfg(feature = "txpool")]
mod txpool;
mod web3;

#[cfg(feature = "txpool")]
pub use self::txpool::TxPool;
pub use self::{
	cache::{EthBlockDataCacheTask, EthTask},
	debug::Debug,
	eth::{format, pending, EstimateGasAdapter, Eth, EthConfig, EthFilter},
	eth_pubsub::{EthPubSub, EthereumSubIdProvider},
	net::Net,
	signer::{EthDevSigner, EthSigner},
	web3::Web3,
};
pub use ethereum::TransactionV3 as EthereumTransaction;
#[cfg(feature = "txpool")]
pub use fc_rpc_core::TxPoolApiServer;
pub use fc_rpc_core::{
	DebugApiServer, EthApiServer, EthFilterApiServer, EthPubSubApiServer, NetApiServer,
	Web3ApiServer,
};
pub use fc_storage::{overrides::*, StorageOverrideHandler};

pub mod frontier_backend_client {
	use super::internal_err;

	use ethereum_types::{H160, H256, U256};
	use jsonrpsee::core::RpcResult;
	use scale_codec::Encode;
	// Substrate
	use sc_client_api::{
		backend::{Backend, StorageProvider},
		StorageKey,
	};
	use sp_blockchain::HeaderBackend;
	use sp_io::hashing::{blake2_128, twox_128};
	use sp_runtime::{
		generic::BlockId,
		traits::{Block as BlockT, HashingFor, UniqueSaturatedInto},
	};
	use sp_state_machine::OverlayedChanges;
	// Frontier
	use fc_rpc_core::types::BlockNumberOrHash;

	/// Implements a default runtime storage override.
	/// It assumes that the balances and nonces are stored in pallet `system.account`, and
	/// have `nonce: Index` = `u32` for  and `free: Balance` = `u128`.
	/// Uses IdentityAddressMapping for the address.
	pub struct SystemAccountId20StorageOverride<B, C, BE>(pub std::marker::PhantomData<(B, C, BE)>);
	impl<B, C, BE> fp_rpc::RuntimeStorageOverride<B, C> for SystemAccountId20StorageOverride<B, C, BE>
	where
		B: BlockT,
		C: StorageProvider<B, BE> + Send + Sync,
		BE: Backend<B>,
	{
		fn is_enabled() -> bool {
			true
		}

		fn set_overlayed_changes(
			client: &C,
			overlayed_changes: &mut OverlayedChanges<HashingFor<B>>,
			block: B::Hash,
			_version: u32,
			address: H160,
			balance: Option<U256>,
			nonce: Option<U256>,
		) {
			let mut key = [twox_128(b"System"), twox_128(b"Account")]
				.concat()
				.to_vec();
			let account_id = Self::into_account_id_bytes(address);
			key.extend(blake2_128(&account_id));
			key.extend(&account_id);

			// Pending calls already carry account changes. Override only the requested
			// fields and preserve the rest of that account's pending state.
			let item = match overlayed_changes.storage(&key) {
				Some(Some(value)) => Some(value.to_owned()),
				// These generic overrides know only the nonce/free offsets, not the
				// runtime's historical AccountData layout. Keep its encoded length,
				// but never restore fields from an account reaped in pending state.
				Some(None) if balance.is_some() || nonce.is_some() => client
					.storage(block, &StorageKey(key.clone()))
					.ok()
					.flatten()
					.map(|item| vec![0; item.0.len()]),
				Some(None) => None,
				None => client
					.storage(block, &StorageKey(key.clone()))
					.ok()
					.flatten()
					.map(|item| item.0),
			};
			if let Some(mut new_item) = item {
				if let Some(nonce) = nonce {
					new_item.splice(0..4, nonce.low_u32().encode());
				}

				if let Some(balance) = balance {
					new_item.splice(16..32, balance.low_u128().encode());
				}

				overlayed_changes.set_storage(key, Some(new_item));
			}
		}

		fn into_account_id_bytes(address: H160) -> Vec<u8> {
			use pallet_evm::AddressMapping;
			let address: H160 = pallet_evm::IdentityAddressMapping::into_account_id(address);
			address.as_ref().to_owned()
		}
	}

	/// Implements a runtime storage override.
	/// It assumes that the balances and nonces are stored in pallet `system.account`, and
	/// have `nonce: Index` = `u32` for  and `free: Balance` = `u128`.
	/// USes HashedAddressMapping for the address.
	pub struct SystemAccountId32StorageOverride<B, C, BE>(pub std::marker::PhantomData<(B, C, BE)>);
	impl<B, C, BE> fp_rpc::RuntimeStorageOverride<B, C> for SystemAccountId32StorageOverride<B, C, BE>
	where
		B: BlockT,
		C: StorageProvider<B, BE> + Send + Sync,
		BE: Backend<B>,
	{
		fn is_enabled() -> bool {
			true
		}

		fn set_overlayed_changes(
			client: &C,
			overlayed_changes: &mut OverlayedChanges<HashingFor<B>>,
			block: B::Hash,
			_version: u32,
			address: H160,
			balance: Option<U256>,
			nonce: Option<U256>,
		) {
			let mut key = [twox_128(b"System"), twox_128(b"Account")]
				.concat()
				.to_vec();
			let account_id = Self::into_account_id_bytes(address);
			key.extend(blake2_128(&account_id));
			key.extend(&account_id);

			// Pending calls already carry account changes. Override only the requested
			// fields and preserve the rest of that account's pending state.
			let item = match overlayed_changes.storage(&key) {
				Some(Some(value)) => Some(value.to_owned()),
				// These generic overrides know only the nonce/free offsets, not the
				// runtime's historical AccountData layout. Keep its encoded length,
				// but never restore fields from an account reaped in pending state.
				Some(None) if balance.is_some() || nonce.is_some() => client
					.storage(block, &StorageKey(key.clone()))
					.ok()
					.flatten()
					.map(|item| vec![0; item.0.len()]),
				Some(None) => None,
				None => client
					.storage(block, &StorageKey(key.clone()))
					.ok()
					.flatten()
					.map(|item| item.0),
			};
			if let Some(mut new_item) = item {
				if let Some(nonce) = nonce {
					new_item.splice(0..4, nonce.low_u32().encode());
				}

				if let Some(balance) = balance {
					new_item.splice(16..32, balance.low_u128().encode());
				}

				overlayed_changes.set_storage(key, Some(new_item));
			}
		}

		fn into_account_id_bytes(address: H160) -> Vec<u8> {
			use pallet_evm::AddressMapping;
			use sp_core::crypto::ByteArray;
			use sp_runtime::traits::BlakeTwo256;

			pallet_evm::HashedAddressMapping::<BlakeTwo256>::into_account_id(address)
				.as_slice()
				.to_owned()
		}
	}

	pub async fn native_block_id<B, C>(
		client: &C,
		backend: &dyn fc_api::Backend<B>,
		number: Option<BlockNumberOrHash>,
	) -> RpcResult<Option<BlockId<B>>>
	where
		B: BlockT,
		C: HeaderBackend<B> + 'static,
	{
		Ok(match number.unwrap_or(BlockNumberOrHash::Latest) {
			BlockNumberOrHash::Hash { hash, .. } => {
				if let Ok(Some(hash)) = load_hash::<B, C>(client, backend, hash).await {
					Some(BlockId::Hash(hash))
				} else {
					None
				}
			}
			BlockNumberOrHash::Num(number) => Some(BlockId::Number(number.unique_saturated_into())),
			BlockNumberOrHash::Latest => match backend.latest_block_hash().await {
				Ok(hash) => Some(BlockId::Hash(hash)),
				Err(e) => {
					log::warn!(target: "rpc", "Failed to get latest block hash from the sql db: {:?}", e);
					Some(BlockId::Hash(client.info().best_hash))
				}
			},
			BlockNumberOrHash::Earliest => Some(BlockId::Hash(client.info().genesis_hash)),
			BlockNumberOrHash::Pending => None,
			BlockNumberOrHash::Safe => Some(BlockId::Hash(client.info().finalized_hash)),
			BlockNumberOrHash::Finalized => Some(BlockId::Hash(client.info().finalized_hash)),
		})
	}

	pub async fn load_hash<B, C>(
		client: &C,
		backend: &dyn fc_api::Backend<B>,
		hash: H256,
	) -> RpcResult<Option<B::Hash>>
	where
		B: BlockT,
		C: HeaderBackend<B> + 'static,
	{
		let substrate_hashes = backend
			.block_hash(&hash)
			.await
			.map_err(|err| internal_err(format!("fetch aux store failed: {:?}", err)))?;

		if let Some(substrate_hashes) = substrate_hashes {
			for substrate_hash in substrate_hashes {
				if is_canon::<B, C>(client, substrate_hash) {
					return Ok(Some(substrate_hash));
				}
			}
		}
		Ok(None)
	}

	pub fn is_canon<B, C>(client: &C, target_hash: B::Hash) -> bool
	where
		B: BlockT,
		C: HeaderBackend<B> + 'static,
	{
		if let Ok(Some(number)) = client.number(target_hash) {
			if let Ok(Some(hash)) = client.hash(number) {
				return hash == target_hash;
			}
		}
		false
	}

	pub async fn load_transactions<B, C>(
		client: &C,
		backend: &dyn fc_api::Backend<B>,
		transaction_hash: H256,
		only_canonical: bool,
	) -> RpcResult<Option<(H256, u32)>>
	where
		B: BlockT,
		C: HeaderBackend<B> + 'static,
	{
		let transaction_metadata = backend
			.transaction_metadata(&transaction_hash)
			.await
			.map_err(|err| internal_err(format!("fetch aux store failed: {:?}", err)))?;

		transaction_metadata
			.iter()
			.find(|meta| is_canon::<B, C>(client, meta.substrate_block_hash))
			.map_or_else(
				|| {
					if !only_canonical && transaction_metadata.len() > 0 {
						Ok(Some((
							transaction_metadata[0].ethereum_block_hash,
							transaction_metadata[0].ethereum_index,
						)))
					} else {
						Ok(None)
					}
				},
				|meta| Ok(Some((meta.ethereum_block_hash, meta.ethereum_index))),
			)
	}
}

pub fn err<T: ToString>(
	code: i32,
	message: T,
	data: Option<&[u8]>,
) -> jsonrpsee::types::error::ErrorObjectOwned {
	jsonrpsee::types::error::ErrorObject::owned(
		code,
		message.to_string(),
		data.map(|bytes| {
			jsonrpsee::core::to_json_raw_value(&format!("0x{}", hex::encode(bytes)))
				.expect("fail to serialize data")
		}),
	)
}

pub fn internal_err<T: ToString>(message: T) -> jsonrpsee::types::error::ErrorObjectOwned {
	err(jsonrpsee::types::error::INTERNAL_ERROR_CODE, message, None)
}

pub fn internal_err_with_data<T: ToString>(
	message: T,
	data: &[u8],
) -> jsonrpsee::types::error::ErrorObjectOwned {
	err(
		jsonrpsee::types::error::INTERNAL_ERROR_CODE,
		message,
		Some(data),
	)
}

pub fn public_key(transaction: &EthereumTransaction) -> Result<[u8; 64], sp_io::EcdsaVerifyError> {
	let mut sig = [0u8; 65];
	let mut msg = [0u8; 32];
	match transaction {
		EthereumTransaction::Legacy(t) => {
			sig[0..32].copy_from_slice(&t.signature.r()[..]);
			sig[32..64].copy_from_slice(&t.signature.s()[..]);
			sig[64] = t.signature.standard_v();
			msg.copy_from_slice(&ethereum::LegacyTransactionMessage::from(t.clone()).hash()[..]);
		}
		EthereumTransaction::EIP2930(t) => {
			sig[0..32].copy_from_slice(&t.signature.r()[..]);
			sig[32..64].copy_from_slice(&t.signature.s()[..]);
			sig[64] = t.signature.odd_y_parity() as u8;
			msg.copy_from_slice(&ethereum::EIP2930TransactionMessage::from(t.clone()).hash()[..]);
		}
		EthereumTransaction::EIP1559(t) => {
			sig[0..32].copy_from_slice(&t.signature.r()[..]);
			sig[32..64].copy_from_slice(&t.signature.s()[..]);
			sig[64] = t.signature.odd_y_parity() as u8;
			msg.copy_from_slice(&ethereum::EIP1559TransactionMessage::from(t.clone()).hash()[..]);
		}
		EthereumTransaction::EIP7702(t) => {
			sig[0..32].copy_from_slice(&t.signature.r()[..]);
			sig[32..64].copy_from_slice(&t.signature.s()[..]);
			sig[64] = t.signature.odd_y_parity() as u8;
			msg.copy_from_slice(&ethereum::EIP7702TransactionMessage::from(t.clone()).hash()[..]);
		}
	}
	sp_io::crypto::secp256k1_ecdsa_recover(&sig, &msg)
}

#[cfg(test)]
mod tests {
	use std::{path::PathBuf, sync::Arc};

	use futures::executor;
	use sc_block_builder::BlockBuilderBuilder;
	use sp_blockchain::HeaderBackend;
	use sp_consensus::BlockOrigin;
	use sp_runtime::{
		generic::{Block, Header},
		traits::{BlakeTwo256, Block as BlockT},
	};
	use substrate_test_runtime_client::{
		prelude::*, DefaultTestClientBuilderExt, TestClientBuilder,
	};
	use tempfile::tempdir;

	type OpaqueBlock =
		Block<Header<u64, BlakeTwo256>, substrate_test_runtime_client::runtime::Extrinsic>;

	#[test]
	fn account_overrides_preserve_pending_fields_and_deletions() {
		use super::frontier_backend_client::{
			SystemAccountId20StorageOverride, SystemAccountId32StorageOverride,
		};
		use fp_rpc::RuntimeStorageOverride;
		use sp_core::{H160, U256};
		use sp_io::hashing::{blake2_128, twox_128};

		let address = H160::repeat_byte(7);

		macro_rules! check {
			($override:ident) => {{
				type Override =
					$override<OpaqueBlock, TestClient, substrate_test_runtime_client::Backend>;
				let account = Override::into_account_id_bytes(address);
				let key = [
					twox_128(b"System").as_slice(),
					twox_128(b"Account").as_slice(),
					blake2_128(&account).as_slice(),
					account.as_slice(),
				]
				.concat();
				let (client, _) = TestClientBuilder::new()
					.add_extra_storage(key.clone(), vec![9; 80])
					.build_with_native_executor::<substrate_test_runtime_client::runtime::RuntimeApi, _>(
						None,
					);
				let at = client.info().best_hash;
				let mut overlay = sp_state_machine::OverlayedChanges::default();
				let pending = vec![7; 80];
				overlay.set_storage(key.clone(), Some(pending.clone()));
				Override::set_overlayed_changes(
					&client,
					&mut overlay,
					at,
					6,
					address,
					Some(U256::from(123)),
					None,
				);
				let value = overlay.storage(&key).unwrap().unwrap();
				assert_eq!(&value[..16], &pending[..16]);
				assert_eq!(&value[16..32], &123u128.to_le_bytes());
				assert_eq!(&value[32..], &pending[32..]);
				Override::set_overlayed_changes(
					&client,
					&mut overlay,
					at,
					6,
					address,
					None,
					Some(U256::from(42)),
				);
				let value = overlay.storage(&key).unwrap().unwrap();
				assert_eq!(&value[..4], &42u32.to_le_bytes());
				assert_eq!(&value[16..32], &123u128.to_le_bytes());
				overlay.set_storage(key.clone(), None);
				Override::set_overlayed_changes(&client, &mut overlay, at, 6, address, None, None);
				assert_eq!(overlay.storage(&key), Some(None));
				Override::set_overlayed_changes(
					&client,
					&mut overlay,
					at,
					6,
					address,
					Some(U256::from(456)),
					None,
				);
				let mut expected = vec![0; 80];
				expected[16..32].copy_from_slice(&456u128.to_le_bytes());
				assert_eq!(overlay.storage(&key), Some(Some(expected.as_slice())));
				overlay.set_storage(key.clone(), None);
				Override::set_overlayed_changes(
					&client,
					&mut overlay,
					at,
					6,
					address,
					None,
					Some(U256::from(42)),
				);
				let mut expected = vec![0; 80];
				expected[..4].copy_from_slice(&42u32.to_le_bytes());
				assert_eq!(overlay.storage(&key), Some(Some(expected.as_slice())));

				// An account absent from parent and overlay keeps the base behavior.
				let mut absent = sp_state_machine::OverlayedChanges::default();
				Override::set_overlayed_changes(
					&client,
					&mut absent,
					at,
					6,
					H160::repeat_byte(8),
					Some(U256::from(456)),
					Some(U256::from(42)),
				);
				assert_eq!(absent.changes().count(), 0);
			}};
		}
		check!(SystemAccountId20StorageOverride);
		check!(SystemAccountId32StorageOverride);
	}

	fn open_frontier_backend<Block: BlockT, C: HeaderBackend<Block>>(
		client: Arc<C>,
		path: PathBuf,
	) -> Result<Arc<fc_db::kv::Backend<Block, C>>, String> {
		Ok(Arc::new(fc_db::kv::Backend::<Block, C>::new(
			client,
			&fc_db::kv::DatabaseSettings {
				source: sc_client_db::DatabaseSource::RocksDb {
					path,
					cache_size: 0,
				},
			},
		)?))
	}

	#[test]
	fn substrate_block_hash_one_to_many_works() {
		let tmp = tempdir().expect("create a temporary directory");
		let (client, _) = TestClientBuilder::new()
			.build_with_native_executor::<substrate_test_runtime_client::runtime::RuntimeApi, _>(
			None,
		);

		let client = Arc::new(client);

		// Create a temporary frontier secondary DB.
		let backend = open_frontier_backend::<OpaqueBlock, _>(client.clone(), tmp.keep())
			.expect("a temporary db was created");

		// A random ethereum block hash to use
		let ethereum_block_hash = sp_core::H256::random();

		// G -> A1.
		let chain = client.chain_info();
		let mut builder = BlockBuilderBuilder::new(&*client)
			.on_parent_block(chain.best_hash)
			.with_parent_block_number(chain.best_number)
			.build()
			.unwrap();
		builder.push_storage_change(vec![1], None).unwrap();
		let a1 = builder.build().unwrap().block;
		let a1_hash = a1.header.hash();
		executor::block_on(client.import(BlockOrigin::Own, a1)).unwrap();

		// A1 -> B1
		let mut builder = BlockBuilderBuilder::new(&*client)
			.on_parent_block(a1_hash)
			.fetch_parent_block_number(&*client)
			.unwrap()
			.build()
			.unwrap();
		builder.push_storage_change(vec![1], None).unwrap();
		let b1 = builder.build().unwrap().block;
		let b1_hash = b1.header.hash();
		executor::block_on(client.import(BlockOrigin::Own, b1)).unwrap();

		// Map B1
		let commitment = fc_db::kv::MappingCommitment::<OpaqueBlock> {
			block_hash: b1_hash,
			ethereum_block_hash,
			ethereum_transaction_hashes: vec![],
		};
		let _ = backend.mapping().write_hashes(commitment);

		// Expect B1 to be canon
		assert_eq!(
			futures::executor::block_on(super::frontier_backend_client::load_hash(
				client.as_ref(),
				backend.as_ref(),
				ethereum_block_hash
			))
			.unwrap()
			.unwrap(),
			b1_hash,
		);

		// A1 -> B2
		let mut builder = BlockBuilderBuilder::new(&*client)
			.on_parent_block(a1_hash)
			.fetch_parent_block_number(&*client)
			.unwrap()
			.build()
			.unwrap();
		builder.push_storage_change(vec![2], None).unwrap();
		let b2 = builder.build().unwrap().block;
		let b2_hash = b2.header.hash();
		executor::block_on(client.import(BlockOrigin::Own, b2)).unwrap();

		// Map B2 to same ethereum hash
		let commitment = fc_db::kv::MappingCommitment::<OpaqueBlock> {
			block_hash: b2_hash,
			ethereum_block_hash,
			ethereum_transaction_hashes: vec![],
		};
		let _ = backend.mapping().write_hashes(commitment);

		// Still expect B1 to be canon
		assert_eq!(
			futures::executor::block_on(super::frontier_backend_client::load_hash(
				client.as_ref(),
				backend.as_ref(),
				ethereum_block_hash
			))
			.unwrap()
			.unwrap(),
			b1_hash,
		);

		// B2 -> C1. B2 branch is now canon.
		let mut builder = BlockBuilderBuilder::new(&*client)
			.on_parent_block(b2_hash)
			.fetch_parent_block_number(&*client)
			.unwrap()
			.build()
			.unwrap();
		builder.push_storage_change(vec![1], None).unwrap();
		let c1 = builder.build().unwrap().block;
		executor::block_on(client.import(BlockOrigin::Own, c1)).unwrap();

		// Expect B2 to be new canon
		assert_eq!(
			futures::executor::block_on(super::frontier_backend_client::load_hash(
				client.as_ref(),
				backend.as_ref(),
				ethereum_block_hash
			))
			.unwrap()
			.unwrap(),
			b2_hash,
		);
	}
}
