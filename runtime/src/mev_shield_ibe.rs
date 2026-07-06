use ark_ec::Group;
use ark_ff::{Field, One, Zero};
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use codec::Decode;
use core::ops::Mul;
use frame_support::dispatch::{DispatchInfo, GetDispatchInfo};
use pallet_shield::{
    DecryptedExtrinsicExecutor, IbeAppliedExtrinsic, IbeDecryptOutcome, IbeEncryptedTxDecryptor,
    IbeKeyVerifier,
};
use sp_core::H256;
use sp_runtime::{DispatchError, traits::UniqueSaturatedInto};
use sp_std::{vec, vec::Vec};
use stp_mev_shield_ibe::{
    BoundedIdentityKey, IbeEncryptedExtrinsicV1, IbeEpochPublicKey, IbePartialDecryptionKeyShareV1,
    MEV_SHIELD_IBE_VERSION, block_identity_bytes,
};
use tle::{
    curves::drand::TinyBLS381,
    ibe::fullident::Identity,
    stream_ciphers::AESGCMStreamCipherProvider,
    tlock::{TLECiphertext, tld},
};
use w3f_bls::EngineBLS;

use crate::{Executive, Runtime, RuntimeCall, UncheckedExtrinsic};

type IbeRuntimeScalar = <TinyBLS381 as EngineBLS>::Scalar;
type IbeRuntimePublicShare = <TinyBLS381 as EngineBLS>::PublicKeyGroup;
type IbeRuntimeIdentityKeyShare = <TinyBLS381 as EngineBLS>::SignatureGroup;

fn ibe_lagrange_coeff_at_zero(id: u32, ids: &[u32]) -> Option<IbeRuntimeScalar> {
    let x_i = IbeRuntimeScalar::from(u64::from(id));
    let mut num = IbeRuntimeScalar::one();
    let mut den = IbeRuntimeScalar::one();
    for other in ids {
        if *other == id {
            continue;
        }
        let x_j = IbeRuntimeScalar::from(u64::from(*other));
        num *= -x_j;
        den *= x_i - x_j;
    }
    den.inverse().map(|inverse| num * inverse)
}
pub struct MevShieldIbeVerifier;

impl<HashT> IbeKeyVerifier<HashT> for MevShieldIbeVerifier
where
    HashT: Into<H256>,
{
    fn verify_block_identity_key(
        genesis_hash: HashT,
        epoch_key: &IbeEpochPublicKey,
        target_block: u64,
        identity_decryption_key: &[u8],
    ) -> bool {
        let genesis_hash: H256 = genesis_hash.into();
        let identity_bytes = block_identity_bytes(
            genesis_hash,
            epoch_key.epoch,
            target_block,
            epoch_key.key_id,
        );
        let identity = Identity::new(stp_mev_shield_ibe::IBE_DOMAIN, vec![identity_bytes]);
        let Ok(master_public_key) =
            IbeRuntimePublicShare::deserialize_compressed(&mut &epoch_key.master_public_key[..])
        else {
            return false;
        };
        let Ok(identity_key) =
            IbeRuntimeIdentityKeyShare::deserialize_compressed(&mut &identity_decryption_key[..])
        else {
            return false;
        };
        let q_id = identity.public::<TinyBLS381>();
        let g2 = <IbeRuntimePublicShare as Group>::generator();
        TinyBLS381::pairing(master_public_key, q_id) == TinyBLS381::pairing(g2, identity_key)
    }

    fn verify_partial_identity_key(
        genesis_hash: HashT,
        epoch_key: &IbeEpochPublicKey,
        share: &IbePartialDecryptionKeyShareV1,
    ) -> bool {
        if share.epoch != epoch_key.epoch || share.key_id != epoch_key.key_id {
            return false;
        }
        let genesis_hash: H256 = genesis_hash.into();
        let identity_bytes =
            block_identity_bytes(genesis_hash, share.epoch, share.target_block, share.key_id);
        let identity = Identity::new(stp_mev_shield_ibe::IBE_DOMAIN, vec![identity_bytes]);
        let Ok(public_share) =
            IbeRuntimePublicShare::deserialize_compressed(&mut &share.public_share[..])
        else {
            return false;
        };
        let Ok(partial_key) = IbeRuntimeIdentityKeyShare::deserialize_compressed(
            &mut &share.partial_identity_key[..],
        ) else {
            return false;
        };
        let q_id = identity.public::<TinyBLS381>();
        let g2 = <IbeRuntimePublicShare as Group>::generator();
        TinyBLS381::pairing(public_share, q_id) == TinyBLS381::pairing(g2, partial_key)
    }

    fn combine_partial_identity_key_shares(
        _epoch_key: &IbeEpochPublicKey,
        shares: &[IbePartialDecryptionKeyShareV1],
    ) -> Option<BoundedIdentityKey> {
        if shares.is_empty() {
            return None;
        }
        let ids = shares
            .iter()
            .map(|share| share.share_id)
            .collect::<Vec<_>>();
        let mut acc = IbeRuntimeIdentityKeyShare::zero();
        for share in shares {
            let partial = IbeRuntimeIdentityKeyShare::deserialize_compressed(
                &mut &share.partial_identity_key[..],
            )
            .ok()?;
            let lambda = ibe_lagrange_coeff_at_zero(share.share_id, &ids)?;
            acc += partial.mul(lambda);
        }
        let mut identity_decryption_key = Vec::new();
        acc.serialize_compressed(&mut identity_decryption_key)
            .ok()?;
        identity_decryption_key.try_into().ok()
    }
}

pub struct MevShieldIbeDecryptor;

impl IbeEncryptedTxDecryptor<UncheckedExtrinsic> for MevShieldIbeDecryptor {
    fn decrypt(data: &[u8]) -> IbeDecryptOutcome<UncheckedExtrinsic> {
        let Ok(envelope) = IbeEncryptedExtrinsicV1::decode_v2(data) else {
            return IbeDecryptOutcome::InvalidAfterKeyAvailable;
        };

        if envelope.version != MEV_SHIELD_IBE_VERSION {
            return IbeDecryptOutcome::InvalidAfterKeyAvailable;
        }

        let current_block: u64 =
            frame_system::Pallet::<Runtime>::block_number().unique_saturated_into();

        if current_block < envelope.target_block {
            return IbeDecryptOutcome::NotReady;
        }

        let Some(block_key) = pallet_shield::Pallet::<Runtime>::ibe_block_decryption_key(
            envelope.epoch,
            envelope.target_block,
            envelope.key_id,
        ) else {
            return IbeDecryptOutcome::NotReady;
        };

        let Ok(ciphertext) =
            TLECiphertext::<TinyBLS381>::deserialize_compressed(&mut &envelope.ciphertext[..])
        else {
            return IbeDecryptOutcome::InvalidAfterKeyAvailable;
        };

        let Ok(identity_key) = <TinyBLS381 as EngineBLS>::SignatureGroup::deserialize_compressed(
            &mut &block_key.identity_decryption_key[..],
        ) else {
            return IbeDecryptOutcome::InvalidAfterKeyAvailable;
        };

        let Ok(plaintext) = tld::<TinyBLS381, AESGCMStreamCipherProvider>(ciphertext, identity_key)
        else {
            return IbeDecryptOutcome::InvalidAfterKeyAvailable;
        };

        if stp_mev_shield_ibe::plaintext_commitment(&plaintext) != envelope.commitment {
            return IbeDecryptOutcome::InvalidAfterKeyAvailable;
        }

        let Ok(inner_xt) = UncheckedExtrinsic::decode(&mut &plaintext[..]) else {
            return IbeDecryptOutcome::InvalidAfterKeyAvailable;
        };

        IbeDecryptOutcome::Ready(inner_xt)
    }
}

pub struct MevShieldInnerExtrinsicExecutor;
impl DecryptedExtrinsicExecutor<UncheckedExtrinsic> for MevShieldInnerExtrinsicExecutor {
    fn dispatch_info(inner: &UncheckedExtrinsic) -> Option<DispatchInfo> {
        Some(inner.0.function.get_dispatch_info())
    }

    fn apply(inner: UncheckedExtrinsic) -> IbeAppliedExtrinsic {
        let fallback_weight = Self::dispatch_info(&inner)
            .map(|info| info.call_weight)
            .unwrap_or_default();

        match Executive::apply_extrinsic(inner) {
            Ok(Ok(())) => IbeAppliedExtrinsic {
                consumed_weight: fallback_weight,
                success: true,
            },

            Ok(Err(_dispatch_error)) => IbeAppliedExtrinsic {
                consumed_weight: fallback_weight,
                success: false,
            },

            Err(_validity_error) => IbeAppliedExtrinsic {
                consumed_weight: fallback_weight,
                success: false,
            },
        }
    }
}

/// Legacy queue decryptor kept for non-v2 queued payloads. v1 live shielding
/// still uses the existing author-side unshielding path and is not routed here.
pub struct LegacyDeferredRuntimeCallDecryptor;

impl pallet_shield::ExtrinsicDecryptor<RuntimeCall> for LegacyDeferredRuntimeCallDecryptor {
    fn decrypt(_data: &[u8]) -> Result<RuntimeCall, DispatchError> {
        Err(DispatchError::Other(
            "Legacy deferred runtime call decryptor disabled",
        ))
    }
}
