//! Dealerless, stake-weighted epoch-ahead DKG for MeV Shield v2.
//!
//! Protocol summary:
//! 1. Runtime exposes the epoch N+2 authority/stake/HPKE-key plan during epoch N.
//! 2. Each authority creates a Shamir polynomial of degree threshold_atoms - 1.
//! 3. Each authority broadcasts coefficient commitments and encrypted shares for every atom.
//! 4. Recipients decrypt only their own atom shares, verify them against commitments, and vote.
//! 5. Once accepted dealer weight is >= 2/3 of active stake, every validator derives the same
//!    epoch public output and its local secret atoms, persists them, and publishes a threshold
//!    attestation for the epoch public key.

use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Mul,
};

use ark_ec::Group;
use ark_ff::{One, UniformRand, Zero};
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use chacha20poly1305::{AeadCore, KeyInit, XChaCha20Poly1305, XNonce, aead::Aead};
use codec::{Decode, Encode};
use mev_shield_ibe_runtime_api::DkgConsensusSource;
use rand_core::{CryptoRng, RngCore};
use sp_core::{H256, Pair, blake2_256};
use stp_mev_shield_ibe::{
    BoundedDkgPublicShareAtoms, BoundedMasterPublicKey, BoundedPublicShare,
    IbeDkgPublicShareAtomV1, IbeEpochPublicKey, KEY_ID_LEN, MEV_SHIELD_IBE_VERSION,
};
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

use super::crypto::{
    EpochDkgPublicOutput, EpochSecretShareBundle, PublicShare, PublicShareAtom, Scalar,
    WeightedSecretShareAtom,
};
use super::dkg_weighting::{ActiveValidatorStake, DkgAtomPlan, plan_stake_weighted_atoms};

#[subtensor_macros::freeze_struct("4acb8cefc65d3547")]
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Debug, Encode, Decode)]
pub struct DkgRoundId {
    pub epoch: u64,
    pub key_id: [u8; KEY_ID_LEN],
    pub first_block: u64,
    pub last_block: u64,
    pub genesis_hash: H256,
}

#[subtensor_macros::freeze_struct("12be3e6460dcd90d")]
#[derive(Clone, Debug, Encode, Decode)]
pub struct DkgDealerCommitmentV1 {
    pub version: u16,
    pub round: DkgRoundId,
    pub dealer_authority_id: Vec<u8>,
    pub dealer_stake: u128,
    /// Compressed g2^{a_i}, one item per polynomial coefficient.
    pub coefficient_commitments: Vec<Vec<u8>>,
    /// SCALE encoded encrypted shares. Broadcast; only recipient can decrypt its entry.
    pub encrypted_shares: Vec<DkgEncryptedShareV1>,
    /// Signature by the consensus/session authority over `dkg_dealer_commitment_payload_hash`.
    pub authority_signature: Vec<u8>,
}

#[subtensor_macros::freeze_struct("2683623657c7a320")]
#[derive(Clone, Debug, Encode, Decode)]
pub struct DkgEncryptedShareV1 {
    pub sender_authority_id: Vec<u8>,
    pub recipient_authority_id: Vec<u8>,
    pub share_id: u32,
    pub sender_x25519_public_key: [u8; 32],
    pub recipient_x25519_public_key: [u8; 32],
    pub nonce: [u8; 24],
    /// XChaCha20-Poly1305 ciphertext of DkgPlainShareV1.
    pub ciphertext: Vec<u8>,
}

#[subtensor_macros::freeze_struct("2106aa278b64fa3d")]
#[derive(Clone, Debug, Encode, Decode)]
pub struct DkgPlainShareV1 {
    pub version: u16,
    pub round: DkgRoundId,
    pub dealer_authority_id: Vec<u8>,
    pub recipient_authority_id: Vec<u8>,
    pub share_id: u32,
    /// Compressed scalar f_dealer(share_id).
    pub secret_scalar: Vec<u8>,
}

#[subtensor_macros::freeze_struct("15849353b55e082d")]
#[derive(Clone, Debug, Encode, Decode)]
pub struct DkgAcceptanceVoteV1 {
    pub version: u16,
    pub round: DkgRoundId,
    pub voter_authority_id: Vec<u8>,
    pub accepted_dealer_authority_id: Vec<u8>,
    pub vote_hash: H256,
    pub authority_signature: Vec<u8>,
}

#[subtensor_macros::freeze_struct("517ca57d032e750a")]
#[derive(Clone, Debug, Encode, Decode)]
pub struct DkgOutputAttestationV1 {
    pub version: u16,
    pub round: DkgRoundId,
    pub authority_id: Vec<u8>,
    pub stake: u128,
    pub public_output_hash: H256,
    pub authority_signature: Vec<u8>,
}

#[derive(Clone)]
pub struct LocalDkgKeys {
    /// Primary local authority id used when constructing messages.
    pub authority_id: Vec<u8>,
    /// All consensus authority ids controlled by this node.  During the POA->POS
    /// transition the same node may control both Aura and BABE keys; incoming
    /// encrypted shares for either key should be accepted if the runtime plan
    /// selected that key.
    pub authority_ids: Vec<Vec<u8>>,
    pub x25519_secret: StaticSecret,
    pub x25519_public: [u8; 32],
}

#[derive(Clone, Debug)]
pub struct VerifiedDealerShare {
    pub dealer_authority_id: Vec<u8>,
    pub share_id: u32,
    pub scalar: Scalar,
}

#[derive(Default, Clone, Debug)]
pub struct DkgRoundAccumulator {
    pub commitments: BTreeMap<Vec<u8>, DkgDealerCommitmentV1>,
    pub accepted_votes: BTreeMap<(Vec<u8>, Vec<u8>), DkgAcceptanceVoteV1>,
    pub local_verified_shares: BTreeMap<(Vec<u8>, u32), VerifiedDealerShare>,
    pub output_attestations: BTreeMap<Vec<u8>, DkgOutputAttestationV1>,
}

pub fn derive_key_id(
    genesis_hash: H256,
    epoch: u64,
    first_block: u64,
    last_block: u64,
    plan_hash: H256,
) -> [u8; KEY_ID_LEN] {
    let h = blake2_256(
        &(
            b"bittensor.mev-shield.v2.dkg.key-id",
            genesis_hash,
            epoch,
            first_block,
            last_block,
            plan_hash,
        )
            .encode(),
    );
    let mut key_id = [0u8; KEY_ID_LEN];
    key_id.copy_from_slice(&h[..KEY_ID_LEN]);
    key_id
}

pub fn dealer_commitment_payload_hash(msg: &DkgDealerCommitmentV1) -> H256 {
    H256::from(blake2_256(
        &(
            b"bittensor.mev-shield.v2.dkg.dealer-commitment",
            &msg.round,
            &msg.dealer_authority_id,
            msg.dealer_stake,
            &msg.coefficient_commitments,
            &msg.encrypted_shares,
        )
            .encode(),
    ))
}

pub fn acceptance_vote_payload_hash(msg: &DkgAcceptanceVoteV1) -> H256 {
    H256::from(blake2_256(
        &(
            b"bittensor.mev-shield.v2.dkg.acceptance-vote",
            &msg.round,
            &msg.voter_authority_id,
            &msg.accepted_dealer_authority_id,
            msg.vote_hash,
        )
            .encode(),
    ))
}

pub fn epoch_publication_payload_hash(
    epoch: u64,
    key_id: [u8; KEY_ID_LEN],
    first_block: u64,
    last_block: u64,
    consensus_source: DkgConsensusSource,
    master_public_key: &[u8],
    total_weight: u128,
    threshold_weight: u128,
    public_atoms: &[IbeDkgPublicShareAtomV1],
) -> H256 {
    H256::from(blake2_256(
        &(
            b"bittensor.mev-shield.v2.dkg.public-output",
            epoch,
            key_id,
            first_block,
            last_block,
            consensus_source,
            master_public_key,
            total_weight,
            threshold_weight,
            public_atoms,
        )
            .encode(),
    ))
}

pub fn output_attestation_payload_hash(msg: &DkgOutputAttestationV1) -> H256 {
    H256::from(blake2_256(
        &(
            b"bittensor.mev-shield.v2.dkg.output-attestation",
            msg.round.epoch,
            msg.round.key_id,
            msg.public_output_hash,
            &msg.authority_id,
            msg.stake,
        )
            .encode(),
    ))
}

pub fn verify_sr25519_authority_signature(
    authority_id: &[u8],
    payload_hash: H256,
    signature: &[u8],
) -> bool {
    let Ok(public_bytes) = <[u8; 32]>::try_from(authority_id) else {
        return false;
    };
    let Ok(signature_bytes) = <[u8; 64]>::try_from(signature) else {
        return false;
    };
    let public = sp_core::sr25519::Public::from_raw(public_bytes);
    let signature = sp_core::sr25519::Signature::from_raw(signature_bytes);
    sp_core::sr25519::Pair::verify(&signature, payload_hash.as_fixed_bytes(), &public)
}

fn scalar_to_bytes(s: Scalar) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    s.serialize_compressed(&mut out)
        .map_err(|e| format!("serialize scalar: {e:?}"))?;
    Ok(out)
}

pub fn scalar_from_bytes_for_worker(bytes: &[u8]) -> Result<Scalar, String> {
    Scalar::deserialize_compressed(&mut &bytes[..])
        .map_err(|e| format!("deserialize scalar: {e:?}"))
}

fn public_share_to_bytes(p: PublicShare) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    p.serialize_compressed(&mut out)
        .map_err(|e| format!("serialize public share: {e:?}"))?;
    Ok(out)
}

fn public_share_from_bytes(bytes: &[u8]) -> Result<PublicShare, String> {
    PublicShare::deserialize_compressed(&mut &bytes[..])
        .map_err(|e| format!("deserialize public share: {e:?}"))
}

fn evaluate_poly(coefficients: &[Scalar], x: Scalar) -> Scalar {
    let mut acc = Scalar::zero();
    for coeff in coefficients.iter().rev() {
        acc *= x;
        acc += coeff;
    }
    acc
}

fn commitment_eval(commitments: &[PublicShare], x: Scalar) -> PublicShare {
    let mut acc = PublicShare::zero();
    let mut pow = Scalar::one();
    for c in commitments {
        acc += c.mul(pow);
        pow *= x;
    }
    acc
}

fn hpke_key(
    local_secret: &StaticSecret,
    remote_public: [u8; 32],
    round: &DkgRoundId,
    share_id: u32,
) -> [u8; 32] {
    let shared = local_secret.diffie_hellman(&X25519PublicKey::from(remote_public));
    blake2_256(
        &(
            b"bittensor.mev-shield.v2.dkg.hpke",
            shared.as_bytes(),
            round,
            share_id,
        )
            .encode(),
    )
}

pub fn encrypt_share<R: RngCore + CryptoRng>(
    rng: &mut R,
    sender_authority_id: Vec<u8>,
    sender_secret: &StaticSecret,
    sender_public: [u8; 32],
    recipient_public: [u8; 32],
    round: &DkgRoundId,
    plain: &DkgPlainShareV1,
) -> Result<DkgEncryptedShareV1, String> {
    let key = hpke_key(sender_secret, recipient_public, round, plain.share_id);
    let cipher = XChaCha20Poly1305::new((&key).into());
    let nonce = XChaCha20Poly1305::generate_nonce(rng);
    let ciphertext = cipher
        .encrypt(&nonce, plain.encode().as_ref())
        .map_err(|_| "encrypt DKG share".to_string())?;
    let mut nonce_bytes = [0u8; 24];
    nonce_bytes.copy_from_slice(&nonce);
    Ok(DkgEncryptedShareV1 {
        sender_authority_id,
        recipient_authority_id: plain.recipient_authority_id.clone(),
        share_id: plain.share_id,
        sender_x25519_public_key: sender_public,
        recipient_x25519_public_key: recipient_public,
        nonce: nonce_bytes,
        ciphertext,
    })
}

pub fn decrypt_share(
    local_secret: &StaticSecret,
    round: &DkgRoundId,
    encrypted: &DkgEncryptedShareV1,
) -> Result<DkgPlainShareV1, String> {
    let key = hpke_key(
        local_secret,
        encrypted.sender_x25519_public_key,
        round,
        encrypted.share_id,
    );
    let cipher = XChaCha20Poly1305::new((&key).into());
    let plaintext = cipher
        .decrypt(
            XNonce::from_slice(&encrypted.nonce),
            encrypted.ciphertext.as_ref(),
        )
        .map_err(|_| "decrypt DKG share".to_string())?;
    let plain = DkgPlainShareV1::decode(&mut &plaintext[..])
        .map_err(|e| format!("decode DKG plain share: {e:?}"))?;
    if plain.round != *round || plain.share_id != encrypted.share_id {
        return Err("decrypted DKG share does not match envelope".into());
    }
    Ok(plain)
}

pub fn build_dealer_commitment<R: RngCore + CryptoRng>(
    rng: &mut R,
    round: DkgRoundId,
    local: &LocalDkgKeys,
    local_stake: u128,
    atom_plan: &DkgAtomPlan<Vec<u8>>,
    authority_signature: Vec<u8>,
) -> Result<DkgDealerCommitmentV1, String> {
    let threshold_atoms: usize = atom_plan
        .threshold_weight
        .try_into()
        .map_err(|_| "threshold too large")?;
    if threshold_atoms == 0 {
        return Err("threshold is zero".into());
    }

    let coefficients: Vec<Scalar> = (0..threshold_atoms).map(|_| Scalar::rand(rng)).collect();
    let generator = <PublicShare as Group>::generator();
    let coefficient_commitments: Vec<Vec<u8>> = coefficients
        .iter()
        .map(|c| public_share_to_bytes(generator.mul(*c)))
        .collect::<Result<_, _>>()?;

    let encrypted_shares = atom_plan
        .atoms
        .iter()
        .map(|atom| {
            let x = Scalar::from(u64::from(atom.share_id));
            let y = evaluate_poly(&coefficients, x);
            let plain = DkgPlainShareV1 {
                version: MEV_SHIELD_IBE_VERSION,
                round: round.clone(),
                dealer_authority_id: local.authority_id.clone(),
                recipient_authority_id: atom.authority_id.clone(),
                share_id: atom.share_id,
                secret_scalar: scalar_to_bytes(y)?,
            };
            encrypt_share(
                rng,
                local.authority_id.clone(),
                &local.x25519_secret,
                local.x25519_public,
                atom.dkg_x25519_public_key,
                &round,
                &plain,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(DkgDealerCommitmentV1 {
        version: MEV_SHIELD_IBE_VERSION,
        round,
        dealer_authority_id: local.authority_id.clone(),
        dealer_stake: local_stake,
        coefficient_commitments,
        encrypted_shares,
        authority_signature,
    })
}

pub fn verify_plain_share(
    dealer: &DkgDealerCommitmentV1,
    share: &DkgPlainShareV1,
) -> Result<(), String> {
    if dealer.version != MEV_SHIELD_IBE_VERSION || share.version != MEV_SHIELD_IBE_VERSION {
        return Err("bad DKG version".into());
    }
    if dealer.round != share.round || dealer.dealer_authority_id != share.dealer_authority_id {
        return Err("DKG share round/dealer mismatch".into());
    }
    if share.share_id == 0 {
        return Err("share_id 0 is invalid".into());
    }
    let commitments: Vec<PublicShare> = dealer
        .coefficient_commitments
        .iter()
        .map(|c| public_share_from_bytes(c))
        .collect::<Result<_, _>>()?;
    let scalar = scalar_from_bytes_for_worker(&share.secret_scalar)?;
    let lhs = <PublicShare as Group>::generator().mul(scalar);
    let rhs = commitment_eval(&commitments, Scalar::from(u64::from(share.share_id)));
    if lhs != rhs {
        return Err("DKG share does not verify against dealer commitment".into());
    }
    Ok(())
}

pub fn finalize_local_output(
    round: &DkgRoundId,
    atom_plan: &DkgAtomPlan<Vec<u8>>,
    local_authority: &[u8],
    accepted_dealers: &[DkgDealerCommitmentV1],
    local_verified_shares: &[VerifiedDealerShare],
) -> Result<EpochSecretShareBundle, String> {
    let accepted_ids: BTreeSet<Vec<u8>> = accepted_dealers
        .iter()
        .map(|d| d.dealer_authority_id.clone())
        .collect();
    if accepted_ids.is_empty() {
        return Err("no accepted DKG dealers".into());
    }

    let mut master_public = PublicShare::zero();
    for dealer in accepted_dealers {
        let c0 = dealer
            .coefficient_commitments
            .first()
            .ok_or_else(|| "empty dealer commitment".to_string())?;
        master_public += public_share_from_bytes(c0)?;
    }
    let master_public_key: BoundedMasterPublicKey = public_share_to_bytes(master_public)?
        .try_into()
        .map_err(|_| "master public key has wrong length".to_string())?;

    let mut by_share: BTreeMap<u32, Scalar> = BTreeMap::new();
    let mut local_share_ids = BTreeSet::<u32>::new();
    for atom in &atom_plan.atoms {
        if atom.authority_id.as_slice() == local_authority {
            local_share_ids.insert(atom.share_id);
        }
    }
    for share in local_verified_shares {
        if accepted_ids.contains(&share.dealer_authority_id)
            && local_share_ids.contains(&share.share_id)
        {
            by_share
                .entry(share.share_id)
                .and_modify(|s| *s += share.scalar)
                .or_insert(share.scalar);
        }
    }

    let mut public_atoms = Vec::with_capacity(atom_plan.atoms.len());
    let mut local_atoms = Vec::new();
    let generator = <PublicShare as Group>::generator();

    for atom in &atom_plan.atoms {
        let mut public = PublicShare::zero();
        for dealer in accepted_dealers {
            let commitments: Vec<PublicShare> = dealer
                .coefficient_commitments
                .iter()
                .map(|c| public_share_from_bytes(c))
                .collect::<Result<_, _>>()?;
            public += commitment_eval(&commitments, Scalar::from(u64::from(atom.share_id)));
        }
        let public_atom = PublicShareAtom {
            share_id: atom.share_id,
            weight: atom.weight,
            public_share: public_share_to_bytes(public)?,
        };
        if atom.authority_id.as_slice() == local_authority {
            let scalar = by_share
                .get(&atom.share_id)
                .ok_or_else(|| format!("missing local verified share {}", atom.share_id))?;
            let check = generator.mul(*scalar);
            if check != public {
                return Err(format!(
                    "local scalar/public mismatch for share_id {}",
                    atom.share_id
                ));
            }
            local_atoms.push(WeightedSecretShareAtom {
                public: public_atom.clone(),
                secret_scalar: scalar_to_bytes(*scalar)?,
            });
        }
        public_atoms.push(public_atom);
    }

    let epoch_public_atoms_vec = public_atoms
        .iter()
        .map(|atom| -> Result<IbeDkgPublicShareAtomV1, String> {
            let public_share: BoundedPublicShare = atom
                .public_share
                .clone()
                .try_into()
                .map_err(|_| "public DKG atom has wrong length".to_string())?;
            Ok(IbeDkgPublicShareAtomV1 {
                share_id: atom.share_id,
                weight: atom.weight,
                public_share,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let epoch_public_atoms: BoundedDkgPublicShareAtoms = epoch_public_atoms_vec
        .try_into()
        .map_err(|_| "too many DKG public atoms".to_string())?;
    let epoch_key = IbeEpochPublicKey {
        epoch: round.epoch,
        key_id: round.key_id,
        master_public_key,
        total_weight: atom_plan.total_weight,
        threshold_weight: atom_plan.threshold_weight,
        public_atoms: epoch_public_atoms,
        first_block: round.first_block,
        last_block: round.last_block,
    };

    Ok(EpochSecretShareBundle {
        public: EpochDkgPublicOutput {
            epoch_key,
            public_atoms,
        },
        validator_authority: local_authority.to_vec(),
        local_atoms,
    })
}

pub fn plan_from_runtime_authorities(
    authorities: Vec<(Vec<u8>, u128, [u8; 32])>,
    max_atoms: u32,
) -> Result<DkgAtomPlan<Vec<u8>>, String> {
    let validators = authorities
        .into_iter()
        .map(
            |(authority_id, stake, dkg_x25519_public_key)| ActiveValidatorStake {
                authority_id,
                stake,
                dkg_x25519_public_key,
            },
        )
        .collect::<Vec<_>>();
    let plan = plan_stake_weighted_atoms(&validators, max_atoms)
        .map_err(|e| format!("stake-weighted DKG plan failed: {e:?}"))?;
    if plan.threshold_weight == 0 || plan.threshold_weight > plan.total_weight {
        return Err(format!(
            "bad DKG threshold {} for total weight {}",
            plan.threshold_weight, plan.total_weight
        ));
    }
    Ok(plan)
}

pub fn dkg_transport_key_payload_hash(
    round: &DkgRoundId,
    authority_id: &[u8],
    dkg_x25519_public_key: &[u8; 32],
) -> H256 {
    H256::from(blake2_256(
        &(
            b"mev-shield-ibe:dkg-transport-key:v2".as_slice(),
            round,
            authority_id,
            dkg_x25519_public_key,
        )
            .encode(),
    ))
}

#[cfg(test)]
mod mev_shield_dkg_protocol_unit_tests {
    use super::*;

    fn round() -> DkgRoundId {
        DkgRoundId {
            epoch: 7,
            key_id: [9; KEY_ID_LEN],
            first_block: 700,
            last_block: 799,
            genesis_hash: H256::repeat_byte(0x42),
        }
    }

    fn encrypted_share(share_id: u32) -> DkgEncryptedShareV1 {
        DkgEncryptedShareV1 {
            sender_authority_id: b"dealer".to_vec(),
            recipient_authority_id: b"recipient".to_vec(),
            share_id,
            sender_x25519_public_key: [1; 32],
            recipient_x25519_public_key: [2; 32],
            nonce: [3; 24],
            ciphertext: vec![share_id as u8; 8],
        }
    }

    #[test]
    fn key_id_is_domain_separated_by_epoch_window_and_plan_hash() {
        let genesis = H256::repeat_byte(1);
        let a = derive_key_id(genesis, 10, 100, 199, H256::repeat_byte(2));
        assert_ne!(
            a,
            derive_key_id(genesis, 11, 100, 199, H256::repeat_byte(2))
        );
        assert_ne!(
            a,
            derive_key_id(genesis, 10, 101, 199, H256::repeat_byte(2))
        );
        assert_ne!(
            a,
            derive_key_id(genesis, 10, 100, 200, H256::repeat_byte(2))
        );
        assert_ne!(
            a,
            derive_key_id(genesis, 10, 100, 199, H256::repeat_byte(3))
        );
    }

    #[test]
    fn dealer_commitment_hash_ignores_signature_and_binds_payload() {
        let mut msg = DkgDealerCommitmentV1 {
            version: stp_mev_shield_ibe::MEV_SHIELD_IBE_VERSION,
            round: round(),
            dealer_authority_id: b"dealer".to_vec(),
            dealer_stake: 10,
            coefficient_commitments: vec![vec![1, 2, 3]],
            encrypted_shares: vec![encrypted_share(1)],
            authority_signature: vec![1; 64],
        };

        let original = dealer_commitment_payload_hash(&msg);
        msg.authority_signature = vec![2; 64];
        assert_eq!(original, dealer_commitment_payload_hash(&msg));

        msg.encrypted_shares.push(encrypted_share(2));
        assert_ne!(original, dealer_commitment_payload_hash(&msg));
    }

    #[test]
    fn acceptance_vote_hash_ignores_signature_and_binds_voter_dealer_and_hash() {
        let mut vote = DkgAcceptanceVoteV1 {
            version: stp_mev_shield_ibe::MEV_SHIELD_IBE_VERSION,
            round: round(),
            voter_authority_id: b"voter".to_vec(),
            accepted_dealer_authority_id: b"dealer".to_vec(),
            vote_hash: H256::repeat_byte(9),
            authority_signature: vec![1; 64],
        };

        let original = acceptance_vote_payload_hash(&vote);
        vote.authority_signature = vec![2; 64];
        assert_eq!(original, acceptance_vote_payload_hash(&vote));

        vote.accepted_dealer_authority_id = b"other".to_vec();
        assert_ne!(original, acceptance_vote_payload_hash(&vote));
    }

    #[test]
    fn output_publication_hash_binds_all_public_parameters() {
        let key_id = [1; KEY_ID_LEN];
        let hash = epoch_publication_payload_hash(
            7,
            key_id,
            70,
            79,
            DkgConsensusSource::PoaAuraRootValidators,
            &[8; 96],
            100,
            67,
            &[],
        );

        assert_ne!(
            hash,
            epoch_publication_payload_hash(
                7,
                key_id,
                70,
                79,
                DkgConsensusSource::PosBabeRootValidators,
                &[8; 96],
                100,
                67,
                &[]
            )
        );
        assert_ne!(
            hash,
            epoch_publication_payload_hash(
                7,
                key_id,
                70,
                79,
                DkgConsensusSource::PoaAuraRootValidators,
                &[9; 96],
                100,
                67,
                &[]
            )
        );
    }
}
