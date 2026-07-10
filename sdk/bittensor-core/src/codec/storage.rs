//! Storage key construction and map-key recovery.
//!
//! A storage item's key is
//! `twox128(pallet_prefix) ++ twox128(item_name) ++ hashed(param)...`.
//! Reversible hashers (Blake2_128Concat / Twox64Concat / Identity) carry the
//! raw key material after the hash, which is how map keys decode straight
//! from the storage key.

// Client-side codec, not runtime code: indexes are guarded by the length
// checks above them.
#![allow(clippy::indexing_slicing)]

use sp_core::hashing::{blake2_128, blake2_256, twox_128, twox_64};

use crate::codec::decode::Cursor;
use crate::codec::value::Value;
use crate::error::CoreError;
use crate::runtime::{Runtime, StorageInfo};

/// Apply one metadata-named storage hasher.
pub fn hash_param(hasher: &str, data: &[u8]) -> Result<Vec<u8>, CoreError> {
    Ok(match hasher {
        "Blake2_256" => blake2_256(data).to_vec(),
        "Blake2_128" => blake2_128(data).to_vec(),
        "Blake2_128Concat" => {
            let mut out = blake2_128(data).to_vec();
            out.extend_from_slice(data);
            out
        }
        "Twox256" => sp_core::hashing::twox_256(data).to_vec(),
        "Twox128" => twox_128(data).to_vec(),
        "Twox64Concat" => {
            let mut out = twox_64(data).to_vec();
            out.extend_from_slice(data);
            out
        }
        "Identity" => data.to_vec(),
        other => {
            return Err(CoreError::Codec(format!(
                "unknown storage hasher {other:?}"
            )))
        }
    })
}

/// How many hash bytes precede the raw key material, for hashers whose keys
/// are recoverable from the storage key.
pub fn concat_hash_len(hasher: &str) -> Result<usize, CoreError> {
    match hasher {
        "Blake2_128Concat" => Ok(16),
        "Twox64Concat" => Ok(8),
        "Identity" => Ok(0),
        other => Err(CoreError::Codec(format!(
            "cannot recover map keys hashed with {other:?}"
        ))),
    }
}

/// `twox128(prefix) ++ twox128(name)` — the 32-byte item prefix.
pub fn storage_prefix(entry: &StorageInfo) -> Vec<u8> {
    let mut out = twox_128(entry.prefix.as_bytes()).to_vec();
    out.extend_from_slice(&twox_128(entry.name.as_bytes()));
    out
}

impl Runtime {
    /// The full storage key for one item; `params` may be a partial prefix
    /// (for map iteration).
    pub fn storage_key(&self, entry: &StorageInfo, params: &[Value]) -> Result<Vec<u8>, CoreError> {
        if params.len() > entry.key_types.len() {
            return Err(CoreError::Codec(format!(
                "{}.{} accepts at most {} parameters, {} given",
                entry.prefix,
                entry.name,
                entry.key_types.len(),
                params.len()
            )));
        }
        let mut key = storage_prefix(entry);
        for (index, param) in params.iter().enumerate() {
            let hasher = entry.hashers.get(index).ok_or_else(|| {
                CoreError::Codec(format!(
                    "{}.{} metadata declares no hasher for param #{}",
                    entry.prefix,
                    entry.name,
                    index.saturating_add(1)
                ))
            })?;
            let mut encoded = Vec::new();
            self.encode_id(entry.key_types[index], param, &mut encoded)?;
            key.extend_from_slice(&hash_param(hasher, &encoded)?);
        }
        Ok(key)
    }

    /// Recover the free map-key components from one full storage key.
    ///
    /// `fixed` is how many leading params were fixed in the queried prefix;
    /// the remaining components decode from the key's trailing bytes
    /// (reversible hashers only).
    pub fn decode_storage_key_params(
        &self,
        entry: &StorageInfo,
        key: &[u8],
        fixed: usize,
    ) -> Result<Vec<Value>, CoreError> {
        let mut prefix_len = 32usize; // two twox128 halves
        for index in 0..fixed {
            let hasher = entry.hashers.get(index).ok_or_else(|| {
                CoreError::Codec(format!(
                    "{}.{} metadata declares no hasher for param #{}",
                    entry.prefix,
                    entry.name,
                    index.saturating_add(1)
                ))
            })?;
            // Fixed params contribute their full hashed length; for
            // non-reversible hashers that length is fixed too.
            let encoded_len = match hasher.as_str() {
                "Blake2_256" | "Twox256" => 32,
                "Blake2_128" | "Twox128" => 16,
                _ => {
                    // Reversible: hash prefix + the raw material; we cannot
                    // know the material length without decoding, so decode it.
                    let hash_len = concat_hash_len(hasher)?;
                    let rest = key
                        .get(prefix_len.saturating_add(hash_len)..)
                        .ok_or_else(|| {
                            CoreError::Codec("storage key shorter than its prefix".into())
                        })?;
                    let mut cursor = Cursor::new(rest);
                    self.decode_id(entry.key_types[index], &mut cursor)?;
                    hash_len.saturating_add(cursor.offset)
                }
            };
            prefix_len = prefix_len.saturating_add(encoded_len);
        }

        let trailing = key
            .get(prefix_len..)
            .ok_or_else(|| CoreError::Codec("storage key shorter than its prefix".into()))?;
        let mut cursor = Cursor::new(trailing);
        let mut out = Vec::new();
        for index in fixed..entry.key_types.len() {
            let hasher = entry.hashers.get(index).ok_or_else(|| {
                CoreError::Codec(format!(
                    "{}.{} metadata declares no hasher for param #{}",
                    entry.prefix,
                    entry.name,
                    index.saturating_add(1)
                ))
            })?;
            let skip = concat_hash_len(hasher)?;
            cursor.take(skip)?;
            out.push(self.decode_id(entry.key_types[index], &mut cursor)?);
        }
        if cursor.remaining() != 0 {
            return Err(CoreError::Codec(format!(
                "{} undecoded bytes remain in storage key",
                cursor.remaining()
            )));
        }
        Ok(out)
    }
}
