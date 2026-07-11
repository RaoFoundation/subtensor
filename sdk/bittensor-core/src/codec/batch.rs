//! Bulk decode entry points — the read-heavy hot loops behind `query_map`
//! pages and runtime-API batches.
//!
//! Decoding is embarrassingly parallel (each item is an independent SCALE
//! blob against an immutable `Runtime`), so batches above
//! [`PARALLEL_THRESHOLD`] fan out across rayon workers. Callers on an FFI
//! boundary release their language's locks around these calls; only
//! host-object materialization stays on the caller's side.

// Indexing is safe by construction: run_indexed only produces in-bounds
// indices for the slices captured by each closure.
#![allow(clippy::indexing_slicing)]

#[cfg(feature = "host")]
use rayon::prelude::*;

use crate::codec::value::Value;
use crate::error::CoreError;
use crate::runtime::type_string::TypeSpec;
use crate::runtime::{Runtime, StorageInfo};

/// Below this many items the rayon fan-out costs more than it saves.
pub const PARALLEL_THRESHOLD: usize = 64;

/// One decoded map entry: the free key components and the value.
pub type DecodedPair = (Vec<Value>, Value);

/// Run `f` over `0..len`, in parallel above the threshold. Without the
/// `host` feature (e.g. browser WASM, where there are no threads) every
/// batch runs serially; the API and results are identical.
#[cfg(feature = "host")]
fn run_indexed<T: Send>(
    len: usize,
    f: impl Fn(usize) -> Result<T, CoreError> + Sync,
) -> Result<Vec<T>, CoreError> {
    if len >= PARALLEL_THRESHOLD {
        (0..len).into_par_iter().map(&f).collect()
    } else {
        (0..len).map(f).collect()
    }
}

#[cfg(not(feature = "host"))]
fn run_indexed<T: Send>(
    len: usize,
    f: impl Fn(usize) -> Result<T, CoreError> + Sync,
) -> Result<Vec<T>, CoreError> {
    (0..len).map(f).collect()
}

impl Runtime {
    /// Decode a batch of `(type string, SCALE bytes)` pairs. Consecutive
    /// duplicate type strings (the common case: one storage item's page)
    /// resolve to a spec once.
    pub fn decode_batch(
        &self,
        type_strings: &[String],
        datas: &[Vec<u8>],
    ) -> Result<Vec<Value>, CoreError> {
        if type_strings.len() != datas.len() {
            return Err(CoreError::Codec(
                "type_strings and datas must have the same length".into(),
            ));
        }
        let mut specs: Vec<TypeSpec> = Vec::with_capacity(type_strings.len());
        let mut last: Option<(&str, TypeSpec)> = None;
        for type_string in type_strings {
            let spec = match &last {
                Some((s, spec)) if *s == type_string.as_str() => spec.clone(),
                _ => {
                    let spec = self.type_spec(type_string)?;
                    last = Some((type_string.as_str(), spec.clone()));
                    spec
                }
            };
            specs.push(spec);
        }
        run_indexed(datas.len(), |i| {
            self.decode_spec(&specs[i], &datas[i], true)
        })
    }

    /// Decode one page of a storage map: recover the free key components
    /// from each full storage key (`fixed` leading params were part of the
    /// queried prefix) and decode each value.
    pub fn decode_map_page(
        &self,
        entry: &StorageInfo,
        raw_keys: &[Vec<u8>],
        raw_values: &[Vec<u8>],
        fixed: usize,
    ) -> Result<Vec<DecodedPair>, CoreError> {
        if raw_keys.len() != raw_values.len() {
            return Err(CoreError::Codec(
                "raw_keys and raw_values must have the same length".into(),
            ));
        }
        let value_spec = TypeSpec::Id(entry.value_type);
        run_indexed(raw_keys.len(), |i| {
            let params = self.decode_storage_key_params(entry, &raw_keys[i], fixed)?;
            let value = self.decode_spec(&value_spec, &raw_values[i], true)?;
            Ok((params, value))
        })
    }

    /// Like [`Runtime::decode_map_page`], but takes the raw
    /// `state_queryStorageAt` change tuples (`0x`-hex key/value strings;
    /// `None` values — keys deleted between the key listing and the value
    /// fetch — are skipped), so hex parsing also runs in the parallel
    /// section.
    pub fn decode_map_changes(
        &self,
        entry: &StorageInfo,
        changes: &[(String, Option<String>)],
        fixed: usize,
    ) -> Result<Vec<DecodedPair>, CoreError> {
        let value_spec = TypeSpec::Id(entry.value_type);
        let unhex = |s: &str| {
            hex::decode(s.trim_start_matches("0x"))
                .map_err(|e| CoreError::Codec(format!("bad hex in changes: {e}")))
        };
        let present: Vec<(&str, &str)> = changes
            .iter()
            .filter_map(|(k, v)| v.as_deref().map(|v| (k.as_str(), v)))
            .collect();
        run_indexed(present.len(), |i| {
            let (key_hex, value_hex) = present[i];
            let raw_key = unhex(key_hex)?;
            let raw_value = unhex(value_hex)?;
            let params = self.decode_storage_key_params(entry, &raw_key, fixed)?;
            let value = self.decode_spec(&value_spec, &raw_value, true)?;
            Ok((params, value))
        })
    }
}
