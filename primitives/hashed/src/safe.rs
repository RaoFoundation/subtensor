//! Scheme-agnostic composite addresses with no raw Ed25519/Sr25519 signing key.
//!
//! These accounts have ordinary delegation policy. Their address does not prove
//! that the authorities use a particular signature scheme or immutable policy.

use codec::Encode;

pub const MAX_DERIVATION_ATTEMPTS: u8 = 64;
/// Subtensor's signer-count limit, shared with offline SDK address derivation.
pub const MAX_MULTISIG_SIGNATORIES: u32 = 100;
pub const MULTISIG_DOMAIN: &[u8] = b"subtensor/safe-multisig/v1";
pub const DERIVATIVE_DOMAIN: &[u8] = b"subtensor/safe-derivative/v1";
pub const PURE_PROXY_DOMAIN: &[u8] = b"subtensor/safe-pure/v1";

/// Return the first full-width hash of SCALE `(domain, manifest, attempt)`
/// rejected by both legacy raw-key decoders. Attempts are `0..64` in order.
///
/// Callers must bound and validate the manifest before invoking this helper.
/// Exhaustion is an error: never substitute the last candidate or a legacy
/// derivation. Runtime and SDK callers use the same canonical encoding.
pub fn safe_derived_account(domain: &[u8], manifest: &impl Encode) -> Option<[u8; 32]> {
    derive_with(domain, manifest, sp_core::hashing::blake2_256)
}

fn derive_with(
    domain: &[u8],
    manifest: &impl Encode,
    mut hash: impl FnMut(&[u8]) -> [u8; 32],
) -> Option<[u8; 32]> {
    let mut encoded = (domain, manifest).encode();
    encoded.push(0);
    for attempt in 0..MAX_DERIVATION_ATTEMPTS {
        *encoded.last_mut()? = attempt;
        let account = hash(&encoded);
        if crate::mldsa::rejects_legacy_public_keys(&account) {
            return Some(account);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_first_full_hash_and_binds_every_manifest_field() {
        let manifest = (alloc::vec![[1u8; 32], [2u8; 32]], 2u16);
        let result = safe_derived_account(MULTISIG_DOMAIN, &manifest);
        assert_eq!(
            result,
            Some([
                42, 34, 149, 134, 241, 8, 13, 143, 224, 200, 176, 5, 78, 113, 69, 82, 238, 255, 93,
                28, 133, 45, 147, 28, 7, 7, 92, 60, 90, 205, 95, 109
            ])
        );
        let expected = (0..MAX_DERIVATION_ATTEMPTS)
            .map(|attempt| {
                sp_core::hashing::blake2_256(&(MULTISIG_DOMAIN, &manifest, attempt).encode())
            })
            .find(crate::mldsa::rejects_legacy_public_keys);
        assert!(result.is_some());
        assert_eq!(result, expected);
        assert_ne!(result, safe_derived_account(DERIVATIVE_DOMAIN, &manifest));
        assert_ne!(
            result,
            safe_derived_account(MULTISIG_DOMAIN, &(manifest.0.clone(), 1u16))
        );
        assert_ne!(
            result,
            safe_derived_account(MULTISIG_DOMAIN, &(alloc::vec![[1u8; 32], [3u8; 32]], 2u16))
        );
    }

    #[test]
    fn exhaustion_checks_exact_bound_and_never_returns_an_unsafe_candidate() {
        let mut attempts = alloc::vec![];
        let result = derive_with(DERIVATIVE_DOMAIN, &([1u8; 32], 7u16), |encoded| {
            attempts.push(encoded.last().copied());
            // The all-zero encoding decompresses as an Edwards point.
            [0u8; 32]
        });
        assert_eq!(result, None);
        assert_eq!(
            attempts,
            (0..MAX_DERIVATION_ATTEMPTS)
                .map(Some)
                .collect::<alloc::vec::Vec<_>>()
        );
    }

    #[test]
    fn tuple_encoding_is_unambiguous_and_deterministic() {
        assert_ne!(
            safe_derived_account(b"a", &b"bc"),
            safe_derived_account(b"ab", &b"c"),
        );
        for index in 0..128u16 {
            let manifest = ([42u8; 32], index);
            let result = safe_derived_account(DERIVATIVE_DOMAIN, &manifest);
            assert!(
                result.is_some_and(|account| crate::mldsa::rejects_legacy_public_keys(&account))
            );
            assert_eq!(result, safe_derived_account(DERIVATIVE_DOMAIN, &manifest));
        }
    }
}
