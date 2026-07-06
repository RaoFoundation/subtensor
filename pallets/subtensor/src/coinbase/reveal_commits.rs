use super::*;
use ark_serialize::CanonicalDeserialize;
use codec::Decode;
use frame_support::{dispatch, traits::OriginTrait};
use scale_info::prelude::collections::VecDeque;
use subtensor_runtime_common::{MechId, NetUid};
use tle::{
    curves::drand::TinyBLS381,
    stream_ciphers::AESGCMStreamCipherProvider,
    tlock::{TLECiphertext, tld},
};
use w3f_bls::EngineBLS;

/// Contains all necessary information to set weights.
///
/// In the context of commit-reveal v3, this is the payload which should be
/// encrypted, compressed, serialized, and submitted to the `commit_crv3_weights`
/// extrinsic.
#[derive(Encode, Decode)]
#[freeze_struct("b6833b5029be4127")]
pub struct WeightsTlockPayload {
    pub hotkey: Vec<u8>,
    pub uids: Vec<u16>,
    pub values: Vec<u16>,
    pub version_key: u64,
}

/// For the old structure
#[derive(Encode, Decode)]
#[freeze_struct("304e55f41267caa")]
pub struct LegacyWeightsTlockPayload {
    pub uids: Vec<u16>,
    pub values: Vec<u16>,
    pub version_key: u64,
}

impl<T: Config> Pallet<T> {
    /// The `reveal_crv3_commits` function is run at the very beginning of epoch `n`,
    pub fn reveal_crv3_commits_for_subnet(netuid: NetUid) -> dispatch::DispatchResult {
        let reveal_period = Self::get_reveal_period(netuid);
        // If the subnet is deferred past this block the
        // commits are taken once here and the later block(s) become no-ops.
        let cur_epoch = Self::current_epoch_with_lookahead(netuid);

        // Weights revealed must have been committed during epoch `cur_epoch - reveal_period`.
        let reveal_epoch = cur_epoch.saturating_sub(reveal_period);

        // All mechanisms share the same epoch, so the reveal_period/reveal_epoch are also the same
        // Reveal for all mechanisms
        for mecid in 0..MechanismCountCurrent::<T>::get(netuid).into() {
            let netuid_index = Self::get_mechanism_storage_index(netuid, mecid.into());

            // Clean expired commits
            for (epoch, _) in TimelockedWeightCommits::<T>::iter_prefix(netuid_index) {
                if epoch < reveal_epoch {
                    TimelockedWeightCommits::<T>::remove(netuid_index, epoch);
                }
            }

            // No commits to reveal until at least epoch reveal_period.
            if cur_epoch < reveal_period {
                log::trace!("Failed to reveal commit for mechanism {netuid_index} Too early");
                return Ok(());
            }

            let mut entries = TimelockedWeightCommits::<T>::take(netuid_index, reveal_epoch);
            let mut unrevealed = VecDeque::new();

            // Keep popping items off the front of the queue until we successfully reveal a commit.
            while let Some((who, commit_block, serialized_compresssed_commit, round_number)) =
                entries.pop_front()
            {
                // Try to get the round number from pallet_drand.
                let pulse = match pallet_drand::Pulses::<T>::get(round_number) {
                    Some(p) => p,
                    None => {
                        // Round number used was not found on the chain. Skip this commit.
                        log::trace!(
                            "Failed to reveal commit for mechanism {netuid_index} submitted by {who:?} on block {commit_block} due to missing round number {round_number}; will retry every block in reveal epoch."
                        );
                        unrevealed.push_back((
                            who,
                            commit_block,
                            serialized_compresssed_commit,
                            round_number,
                        ));
                        continue;
                    }
                };

                let reader = &mut &serialized_compresssed_commit[..];
                let commit = match TLECiphertext::<TinyBLS381>::deserialize_compressed(reader) {
                    Ok(c) => c,
                    Err(e) => {
                        log::trace!(
                            "Failed to reveal commit for mechanism {netuid_index} submitted by {who:?} due to error deserializing the commit: {e:?}"
                        );
                        continue;
                    }
                };

                let signature_bytes = pulse
                    .signature
                    .strip_prefix(b"0x")
                    .unwrap_or(&pulse.signature);

                let sig_reader = &mut &signature_bytes[..];
                let sig = match <TinyBLS381 as EngineBLS>::SignatureGroup::deserialize_compressed(
                    sig_reader,
                ) {
                    Ok(s) => s,
                    Err(e) => {
                        log::trace!(
                            "Failed to reveal commit for mechanism {netuid_index} submitted by {who:?} due to error deserializing signature from drand pallet: {e:?}"
                        );
                        continue;
                    }
                };

                let decrypted_bytes: Vec<u8> = match tld::<TinyBLS381, AESGCMStreamCipherProvider>(
                    commit, sig,
                ) {
                    Ok(d) => d,
                    Err(e) => {
                        log::trace!(
                            "Failed to reveal commit for mechanism {netuid_index} submitted by {who:?} due to error decrypting the commit: {e:?}"
                        );
                        continue;
                    }
                };

                // ------------------------------------------------------------------
                // Try to decode payload with the new and legacy formats.
                // ------------------------------------------------------------------
                let (uids, values, version_key) = {
                    let mut reader_new = &decrypted_bytes[..];
                    if let Ok(payload) = WeightsTlockPayload::decode(&mut reader_new) {
                        // Verify hotkey matches committer
                        let mut hk_reader = &payload.hotkey[..];
                        match T::AccountId::decode(&mut hk_reader) {
                            Ok(decoded_hotkey) if decoded_hotkey == who => {
                                (payload.uids, payload.values, payload.version_key)
                            }
                            Ok(_) => {
                                log::trace!(
                                    "Failed to reveal commit for mechanism {netuid_index} submitted by {who:?} due to hotkey mismatch in payload"
                                );
                                continue;
                            }
                            Err(e) => {
                                let mut reader_legacy = &decrypted_bytes[..];
                                match LegacyWeightsTlockPayload::decode(&mut reader_legacy) {
                                    Ok(legacy) => (legacy.uids, legacy.values, legacy.version_key),
                                    Err(_) => {
                                        log::trace!(
                                            "Failed to reveal commit for mechanism {netuid_index} submitted by {who:?} due to error deserializing hotkey: {e:?}"
                                        );
                                        continue;
                                    }
                                }
                            }
                        }
                    } else {
                        // Fallback to legacy payload
                        let mut reader_legacy = &decrypted_bytes[..];
                        match LegacyWeightsTlockPayload::decode(&mut reader_legacy) {
                            Ok(legacy) => (legacy.uids, legacy.values, legacy.version_key),
                            Err(e) => {
                                log::trace!(
                                    "Failed to reveal commit for mechanism {netuid_index} submitted by {who:?} due to error deserializing both payload formats: {e:?}"
                                );
                                continue;
                            }
                        }
                    }
                };

                // ------------------------------------------------------------------
                //                          Apply weights
                // ------------------------------------------------------------------
                if let Err(e) = Self::do_set_mechanism_weights(
                    OriginFor::<T>::signed(who.clone()),
                    netuid,
                    MechId::from(mecid),
                    uids,
                    values,
                    version_key,
                ) {
                    log::trace!(
                        "Failed to `do_set_mechanism_weights` for mechanism {netuid_index} submitted by {who:?}: {e:?}"
                    );
                    continue;
                }

                Self::deposit_event(Event::TimelockedWeightsRevealed(netuid_index, who));
            }

            if !unrevealed.is_empty() {
                TimelockedWeightCommits::<T>::insert(netuid_index, reveal_epoch, unrevealed);
            }
        }

        Ok(())
    }
}
