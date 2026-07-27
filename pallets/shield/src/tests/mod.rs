//! Unit tests for `pallet-shield` (MEV shield encrypted extrinsic queue + key rotation).

mod admin_queue_limits;
mod announce_next_key;
mod migrate_clear_v1_storage;
mod store_encrypted;
mod submit_encrypted;
mod try_decode_shielded_tx;
mod try_unshield_tx;
