/*
 * Copyright 2024 by Ideal Labs, LLC
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

//! # Drand Bridge Pallet
//!
//! A pallet to bridge to [drand](drand.love)'s Quicknet, injecting publicly verifiable randomness
//! into the runtime
//!
//! ## Overview
//!
//! Quicknet chain runs in an 'unchained' mode, producing a fresh pulse of randomness every 3s
//! This pallet implements an offchain worker that consumes pulses from quicket and then sends a
//! signed transaction to encode them in the runtime. The runtime uses the optimized arkworks host
//! functions to efficiently verify the pulse.
//!
//! Run `cargo doc --package pallet-drand --open` to view this pallet's documentation.

// We make sure this pallet uses `no_std` for compiling to Wasm.
#![cfg_attr(not(feature = "std"), no_std)]
// Many errors are transformed throughout the pallet.
#![allow(clippy::manual_inspect)]

// Re-export pallet items so that they can be accessed from the crate namespace.
pub use pallet::*;

extern crate alloc;

use alloc::{format, string::String, vec, vec::Vec};
use codec::Encode;
use frame_support::{pallet_prelude::*, traits::Randomness};
use frame_system::{
    offchain::{
        AppCrypto, CreateBare, SendUnsignedTransaction, SignedPayload, Signer, SigningTypes,
    },
    pallet_prelude::BlockNumberFor,
};
use scale_info::prelude::cmp;
use sha2::{Digest, Sha256};
use sp_core::blake2_256;
use sp_runtime::{
    KeyTypeId, Saturating,
    traits::{Hash, One},
    transaction_validity::{InvalidTransaction, TransactionValidity, ValidTransaction},
};

pub mod bls12_381;
pub mod drand_priority;
pub mod migrations;
pub mod types;
pub mod utils;
pub mod verifier;

use types::*;
use verifier::Verifier;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

pub mod weights;

/// the main drand api endpoint
const ENDPOINTS: [&str; 5] = [
    "https://api.drand.sh",
    "https://api2.drand.sh",
    "https://api3.drand.sh",
    "https://drand.cloudflare.com",
    "https://api.drand.secureweb3.com:6875",
];

/// the drand quicknet chain hash
/// quicknet uses 'Tiny' BLS381, with small 48-byte sigs in G1 and 96-byte pubkeys in G2
pub const QUICKNET_CHAIN_HASH: &str =
    "52db9ba70e0cc0f6eaf7803dd07447a1f5477735fd3f661792ba94600c84e971";

const CHAIN_HASH: &str = QUICKNET_CHAIN_HASH;

pub const MAX_PULSES_TO_FETCH: u64 = 50;
pub const MAX_KEPT_PULSES: u64 = 216_000; // 1 week
pub const MAX_REMOVED_PULSES: u64 = 100;

/// Defines application identifier for crypto keys of this module.
///
/// Every module that deals with signatures needs to declare its unique identifier for
/// its crypto keys.
/// When offchain worker is signing transactions it's going to request keys of type
/// `KeyTypeId` from the keystore and use the ones it finds to sign the transaction.
/// The keys can be inserted manually via RPC (see `author_insertKey`).
pub const KEY_TYPE: KeyTypeId = KeyTypeId(*b"drnd");

/// Based on the above `KeyTypeId` we need to generate a pallet-specific crypto type wrappers.
/// We can use from supported crypto kinds (`sr25519`, `ed25519` and `ecdsa`) and augment
/// the types with this pallet-specific identifier.
pub mod crypto {
    use super::KEY_TYPE;
    use sp_core::sr25519::Signature as Sr25519Signature;
    use sp_runtime::{
        MultiSignature, MultiSigner,
        app_crypto::{app_crypto, sr25519},
        traits::Verify,
    };
    app_crypto!(sr25519, KEY_TYPE);

    pub struct TestAuthId;

    // implemented for runtime
    impl frame_system::offchain::AppCrypto<MultiSigner, MultiSignature> for TestAuthId {
        type RuntimeAppPublic = Public;
        type GenericSignature = sp_core::sr25519::Signature;
        type GenericPublic = sp_core::sr25519::Public;
    }

    impl frame_system::offchain::AppCrypto<<Sr25519Signature as Verify>::Signer, Sr25519Signature>
        for TestAuthId
    {
        type RuntimeAppPublic = Public;
        type GenericSignature = sp_core::sr25519::Signature;
        type GenericPublic = sp_core::sr25519::Public;
    }
}

impl<T: SigningTypes> SignedPayload<T>
    for BeaconConfigurationPayload<T::Public, BlockNumberFor<T>>
{
    fn public(&self) -> T::Public {
        self.public.clone()
    }
}

impl<T: SigningTypes> SignedPayload<T> for PulsesPayload<T::Public, BlockNumberFor<T>> {
    fn public(&self) -> T::Public {
        self.public.clone()
    }
}

#[frame_support::pallet]
pub mod pallet {
    use super::*;
    use crate::weights::WeightInfo;
    use frame_system::pallet_prelude::*;

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    #[pallet::config]
    pub trait Config: CreateBare<Call<Self>> + SigningTypes + frame_system::Config {
        /// The identifier type for an offchain worker.
        type AuthorityId: AppCrypto<Self::Public, Self::Signature>;
        /// something that knows how to verify beacon pulses
        type Verifier: Verifier;
        /// A configuration for base priority of unsigned transactions.
        ///
        /// This is exposed so that it can be tuned for particular runtime, when
        /// multiple pallets send unsigned transactions.
        #[pallet::constant]
        type UnsignedPriority: Get<TransactionPriority>;
        /// The maximum number of milliseconds we are willing to wait for the HTTP request to
        /// complete.
        #[pallet::constant]
        type HttpFetchTimeout: Get<u64>;
        /// Weight information for extrinsics in this pallet.
        type WeightInfo: crate::weights::WeightInfo;
    }

    /// the drand beacon configuration
    #[pallet::storage]
    pub type BeaconConfig<T: Config> =
        StorageValue<_, BeaconConfiguration, ValueQuery, DefaultBeaconConfig<T>>;

    #[pallet::type_value]
    pub fn DefaultBeaconConfig<T: Config>() -> BeaconConfiguration {
        BeaconConfiguration {
            public_key: OpaquePublicKey::truncate_from(vec![
                131, 207, 15, 40, 150, 173, 238, 126, 184, 181, 240, 31, 202, 211, 145, 34, 18,
                196, 55, 224, 7, 62, 145, 31, 185, 0, 34, 211, 231, 96, 24, 60, 140, 75, 69, 11,
                106, 10, 108, 58, 198, 165, 119, 106, 45, 16, 100, 81, 13, 31, 236, 117, 140, 146,
                28, 194, 43, 14, 23, 230, 58, 175, 75, 203, 94, 214, 99, 4, 222, 156, 248, 9, 189,
                39, 76, 167, 59, 171, 74, 245, 166, 233, 199, 106, 75, 192, 158, 118, 234, 232,
                153, 30, 245, 236, 228, 90,
            ]),
            period: 3,
            genesis_time: 1_692_803_367,
            hash: BoundedHash::truncate_from(vec![
                82, 219, 155, 167, 14, 12, 192, 246, 234, 247, 128, 61, 208, 116, 71, 161, 245, 71,
                119, 53, 253, 63, 102, 23, 146, 186, 148, 96, 12, 132, 233, 113,
            ]),
            group_hash: BoundedHash::truncate_from(vec![
                244, 119, 213, 200, 159, 33, 161, 124, 134, 58, 127, 147, 124, 106, 109, 21, 133,
                148, 20, 210, 190, 9, 205, 68, 141, 66, 121, 175, 51, 28, 93, 62,
            ]),
            scheme_id: BoundedHash::truncate_from(vec![
                98, 108, 115, 45, 117, 110, 99, 104, 97, 105, 110, 101, 100, 45, 103, 49, 45, 114,
                102, 99, 57, 51, 56, 48,
            ]),
            metadata: Metadata {
                beacon_id: BoundedVec::truncate_from(vec![113, 117, 105, 99, 107, 110, 101, 116]),
            },
        }
    }

    /// Define a maximum length for the migration key
    type MigrationKeyMaxLen = ConstU32<128>;

    /// Storage for migration run status
    #[pallet::storage]
    pub type HasMigrationRun<T: Config> =
        StorageMap<_, Identity, BoundedVec<u8, MigrationKeyMaxLen>, bool, ValueQuery>;

    /// map round number to pulse
    #[pallet::storage]
    pub type Pulses<T: Config> = StorageMap<_, Blake2_128Concat, RoundNumber, Pulse, OptionQuery>;

    #[pallet::storage]
    pub type LastStoredRound<T: Config> = StorageValue<_, RoundNumber, ValueQuery>;

    /// oldest stored round
    #[pallet::storage]
    pub type OldestStoredRound<T: Config> = StorageValue<_, RoundNumber, ValueQuery>;

    /// Defines the block when next unsigned transaction will be accepted.
    ///
    /// To prevent spam of unsigned (and unpaid!) transactions on the network,
    /// we only allow one transaction per block.
    /// This storage entry defines when new transaction is going to be accepted.
    #[pallet::storage]
    pub(super) type NextUnsignedAt<T: Config> = StorageValue<_, BlockNumberFor<T>, ValueQuery>;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// Beacon Configuration has changed.
        BeaconConfigChanged,
        /// Successfully set a new pulse(s).
        NewPulse { rounds: Vec<RoundNumber> },
        /// Oldest Stored Round has been set.
        SetOldestStoredRound(u64),
    }

    #[pallet::error]
    pub enum Error<T> {
        /// The value retrieved was `None` as no value was previously set.
        NoneValue,
        /// There was an attempt to increment the value in storage over `u32::MAX`.
        StorageOverflow,
        /// failed to connect to the
        DrandConnectionFailure,
        /// the pulse is invalid
        UnverifiedPulse,
        /// the round number did not increment
        InvalidRoundNumber,
        /// the pulse could not be verified
        PulseVerificationError,
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        fn offchain_worker(block_number: BlockNumberFor<T>) {
            log::debug!("Drand OCW working on block: {block_number:?}");
            if let Err(e) = Self::fetch_drand_pulse_and_send_unsigned(block_number) {
                log::debug!("Drand: Failed to fetch pulse from drand. {e:?}");
            }
        }
        fn on_runtime_upgrade() -> frame_support::weights::Weight {
            /*let weight = */
            frame_support::weights::Weight::from_parts(0, 0) /*;*/

            //weight = weight.saturating_add(migrations::migrate_set_oldest_round::<T>());

            //weight
        }
    }

    #[pallet::validate_unsigned]
    impl<T: Config> ValidateUnsigned for Pallet<T> {
        type Call = Call<T>;

        /// Validate unsigned call to this module.
        ///
        /// By default unsigned transactions are disallowed, but implementing the validator
        /// here we make sure that some particular calls (the ones produced by offchain worker)
        /// are being whitelisted and marked as valid.
        fn validate_unsigned(_source: TransactionSource, call: &Self::Call) -> TransactionValidity {
            match call {
                Call::set_beacon_config {
                    config_payload: payload,
                    signature,
                } => {
                    let signature = signature.as_ref().ok_or(InvalidTransaction::BadSigner)?;
                    Self::validate_signature_and_parameters(
                        payload,
                        signature,
                        &payload.block_number,
                        &payload.public,
                        None,
                    )
                }
                Call::write_pulse {
                    pulses_payload: payload,
                    signature,
                } => {
                    let signature = signature.as_ref().ok_or(InvalidTransaction::BadSigner)?;
                    let rounds: Vec<RoundNumber> = payload.pulses.iter().map(|p| p.round).collect();
                    Self::validate_signature_and_parameters(
                        payload,
                        signature,
                        &payload.block_number,
                        &payload.public,
                        Some(&rounds),
                    )
                }
                _ => InvalidTransaction::Call.into(),
            }
        }
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Verify and write a pulse from the beacon into the runtime
        #[pallet::call_index(0)]
        #[pallet::weight(T::WeightInfo::write_pulse())]
        pub fn write_pulse(
            origin: OriginFor<T>,
            pulses_payload: PulsesPayload<T::Public, BlockNumberFor<T>>,
            _signature: Option<T::Signature>,
        ) -> DispatchResult {
            ensure_none(origin)?;
            let config = BeaconConfig::<T>::get();

            let mut last_stored_round = LastStoredRound::<T>::get();
            let mut new_rounds = Vec::new();

            let oldest_stored_round = OldestStoredRound::<T>::get();
            let is_first_storage = last_stored_round == 0 && oldest_stored_round == 0;
            let mut first_new_round: Option<RoundNumber> = None;

            for pulse in &pulses_payload.pulses {
                let is_verified = T::Verifier::verify(config.clone(), pulse.clone())
                    .map_err(|_| Error::<T>::PulseVerificationError)?;

                if is_verified {
                    ensure!(
                        pulse.round > last_stored_round,
                        Error::<T>::InvalidRoundNumber
                    );

                    // Store the pulse
                    Pulses::<T>::insert(pulse.round, pulse.clone());

                    // Update last stored round
                    last_stored_round = pulse.round;

                    // Collect the new round
                    new_rounds.push(pulse.round);

                    // Set the first new round if this is the initial storage
                    if is_first_storage && first_new_round.is_none() {
                        first_new_round = Some(pulse.round);
                    }
                }
            }

            // Update LastStoredRound storage
            LastStoredRound::<T>::put(last_stored_round);

            // Set OldestStoredRound if this was the first storage
            if let Some(first_round) = first_new_round {
                OldestStoredRound::<T>::put(first_round);
            }

            // Prune old pulses
            Self::prune_old_pulses(last_stored_round);

            // Update the next unsigned block number
            let current_block = frame_system::Pallet::<T>::block_number();
            <NextUnsignedAt<T>>::put(current_block);

            // Emit event with all new rounds
            if !new_rounds.is_empty() {
                Self::deposit_event(Event::NewPulse { rounds: new_rounds });
            }

            Ok(())
        }
        /// allows the root user to set the beacon configuration
        /// generally this would be called from an offchain worker context.
        /// there is no verification of configurations, so be careful with this.
        ///
        /// * `origin`: the root user
        /// * `config`: the beacon configuration
        #[pallet::call_index(1)]
        #[pallet::weight(T::WeightInfo::set_beacon_config())]
        pub fn set_beacon_config(
            origin: OriginFor<T>,
            config_payload: BeaconConfigurationPayload<T::Public, BlockNumberFor<T>>,
            _signature: Option<T::Signature>,
        ) -> DispatchResult {
            ensure_root(origin)?;
            BeaconConfig::<T>::put(config_payload.config);

            // now increment the block number at which we expect next unsigned transaction.
            let current_block = frame_system::Pallet::<T>::block_number();
            <NextUnsignedAt<T>>::put(current_block.saturating_add(One::one()));

            Self::deposit_event(Event::BeaconConfigChanged {});
            Ok(())
        }

        /// allows the root user to set the oldest stored round
        #[pallet::call_index(2)]
        #[pallet::weight(T::WeightInfo::set_oldest_stored_round())]
        pub fn set_oldest_stored_round(origin: OriginFor<T>, oldest_round: u64) -> DispatchResult {
            ensure_root(origin)?;
            OldestStoredRound::<T>::put(oldest_round);
            Self::deposit_event(Event::SetOldestStoredRound(oldest_round));
            Ok(())
        }
    }
}

impl<T: Config> Pallet<T> {
    /// fetch the latest public pulse from the configured drand beacon
    /// then send a signed transaction to include it on-chain
    fn fetch_drand_pulse_and_send_unsigned(
        block_number: BlockNumberFor<T>,
    ) -> Result<(), &'static str> {
        // Ensure we can send an unsigned transaction
        let next_unsigned_at = NextUnsignedAt::<T>::get();
        if next_unsigned_at > block_number {
            return Err("Drand: Too early to send unsigned transaction");
        }

        let mut last_stored_round = LastStoredRound::<T>::get();
        let latest_unbounded_pulse =
            Self::fetch_drand_latest().map_err(|_| "Failed to query drand")?;
        let latest_pulse = latest_unbounded_pulse
            .try_into_pulse()
            .map_err(|_| "Drand: Received pulse contains invalid data")?;
        let current_round = latest_pulse.round;

        // If last_stored_round is zero, set it to current_round - 1
        if last_stored_round == 0 {
            last_stored_round = current_round.saturating_sub(1);
            LastStoredRound::<T>::put(last_stored_round);
        }

        if current_round > last_stored_round {
            let rounds_to_fetch = cmp::min(
                current_round.saturating_sub(last_stored_round),
                MAX_PULSES_TO_FETCH,
            );
            let mut pulses = Vec::new();

            for round in (last_stored_round.saturating_add(1))
                ..=(last_stored_round.saturating_add(rounds_to_fetch))
            {
                let unbounded_pulse = Self::fetch_drand_by_round(round)
                    .map_err(|_| "Drand: Failed to query drand for round")?;
                let pulse = unbounded_pulse
                    .try_into_pulse()
                    .map_err(|_| "Drand: Received pulse contains invalid data")?;
                pulses.push(pulse);
            }

            let signer = Signer::<T, T::AuthorityId>::any_account();

            // Submit one tx per pulse, ascending rounds.
            for pulse in pulses.into_iter() {
                let round = pulse.round;

                if let Some((acc, res)) = signer.send_unsigned_transaction(
                    |account| PulsesPayload {
                        block_number,
                        pulses: vec![pulse.clone()],
                        public: account.public.clone(),
                    },
                    |pulses_payload, signature| Call::write_pulse {
                        pulses_payload,
                        signature: Some(signature),
                    },
                ) {
                    match res {
                        Ok(()) => log::debug!("Drand: [{:?}] submitted round {:?}", acc.id, round),
                        Err(e) => log::debug!(
                            "Drand: [{:?}] failed to submit round {:?}: {:?}",
                            acc.id,
                            round,
                            e
                        ),
                    }
                } else {
                    log::debug!("Drand: No local account available to submit round {round:?}");
                }
            }
        }

        Ok(())
    }

    fn fetch_drand_by_round(round: RoundNumber) -> Result<DrandResponseBody, &'static str> {
        let relative_path = format!("/{CHAIN_HASH}/public/{round}");
        Self::fetch_and_decode_from_any_endpoint(&relative_path)
    }

    fn fetch_drand_latest() -> Result<DrandResponseBody, &'static str> {
        let relative_path = format!("/{CHAIN_HASH}/public/latest");
        Self::fetch_and_decode_from_any_endpoint(&relative_path)
    }

    /// Try to fetch from multiple endpoints simultaneously and return the first successfully decoded JSON response.
    fn fetch_and_decode_from_any_endpoint(
        relative_path: &str,
    ) -> Result<DrandResponseBody, &'static str> {
        let uris: Vec<String> = ENDPOINTS
            .iter()
            .map(|e| format!("{e}{relative_path}"))
            .collect();
        let deadline = sp_io::offchain::timestamp().add(
            sp_runtime::offchain::Duration::from_millis(T::HttpFetchTimeout::get()),
        );

        let mut pending_requests: Vec<(String, sp_runtime::offchain::http::PendingRequest)> =
            vec![];

        // Try sending requests to all endpoints.
        for uri in &uris {
            let request = sp_runtime::offchain::http::Request::get(uri);
            match request.deadline(deadline).send() {
                Ok(pending_req) => {
                    pending_requests.push((uri.clone(), pending_req));
                }
                Err(_) => {
                    log::warn!("Drand: HTTP IO Error on endpoint {uri}");
                }
            }
        }

        if pending_requests.is_empty() {
            log::warn!("Drand: No endpoints could be queried");
            return Err("Drand: No endpoints could be queried");
        }

        loop {
            let now = sp_io::offchain::timestamp();
            if now > deadline {
                // We've passed our deadline without getting a valid response.
                log::warn!("Drand: HTTP Deadline Reached");
                break;
            }

            let mut still_pending = false;
            let mut next_iteration_requests = Vec::new();

            for (uri, request) in pending_requests.drain(..) {
                match request.try_wait(Some(deadline)) {
                    Ok(Ok(response)) => {
                        if response.code != 200 {
                            log::warn!(
                                "Drand: Unexpected status code: {} from {}",
                                response.code,
                                uri
                            );
                            continue;
                        }

                        let body = response.body().collect::<Vec<u8>>();
                        match serde_json::from_slice::<DrandResponseBody>(&body) {
                            Ok(decoded) => {
                                return Ok(decoded);
                            }
                            Err(e) => {
                                log::warn!(
                                    "Drand: JSON decode error from {}: {}. Response body: {}",
                                    uri,
                                    e,
                                    String::from_utf8_lossy(&body)
                                );
                            }
                        }
                    }
                    Ok(Err(e)) => {
                        log::warn!("Drand: HTTP error from {uri}: {e:?}");
                    }
                    Err(pending_req) => {
                        still_pending = true;
                        next_iteration_requests.push((uri, pending_req));
                    }
                }
            }

            pending_requests = next_iteration_requests;

            if !still_pending {
                break;
            }
        }

        // If we reached here, no valid response was obtained from any endpoint.
        log::warn!("Drand: No valid response from any endpoint");
        Err("Drand: No valid response from any endpoint")
    }

    /// get the randomness at a specific block height
    /// returns [0u8;32] if it does not exist
    pub fn random_at(round: RoundNumber) -> [u8; 32] {
        let pulse = Pulses::<T>::get(round).unwrap_or_default();
        let rand = pulse.randomness.clone();
        let bounded_rand: [u8; 32] = rand.into_inner().try_into().unwrap_or([0u8; 32]);

        bounded_rand
    }

    fn validate_signature_and_parameters(
        payload: &impl SignedPayload<T>,
        signature: &T::Signature,
        block_number: &BlockNumberFor<T>,
        public: &T::Public,
        rounds: Option<&[RoundNumber]>,
    ) -> TransactionValidity {
        let signature_valid =
            SignedPayload::<T>::verify::<T::AuthorityId>(payload, signature.clone());
        if !signature_valid {
            return InvalidTransaction::BadProof.into();
        }
        Self::validate_transaction_parameters(block_number, public, rounds)
    }

    fn validate_transaction_parameters(
        block_number: &BlockNumberFor<T>,
        public: &T::Public,
        rounds: Option<&[RoundNumber]>,
    ) -> TransactionValidity {
        let next_unsigned_at = NextUnsignedAt::<T>::get();
        let current_block = frame_system::Pallet::<T>::block_number();

        if current_block < *block_number {
            return InvalidTransaction::Future.into();
        }

        match rounds {
            Some(rs) => {
                let r_opt = rs.first().copied();
                let has_second = rs.get(1).is_some();
                let r = match (r_opt, has_second) {
                    (Some(round), false) => round,
                    _ => return InvalidTransaction::Call.into(),
                };

                // Allow multiple unsigned txs in the same block even after the first updates the gate.
                if next_unsigned_at > current_block {
                    return InvalidTransaction::Stale.into();
                }

                // Drop stale rounds at mempool time to avoid re-including last block's rounds.
                let last = LastStoredRound::<T>::get();
                if r <= last {
                    return InvalidTransaction::Stale.into();
                }

                // Priority favors lower rounds first.
                let priority =
                    T::UnsignedPriority::get().saturating_add(u64::MAX.saturating_sub(r));

                ValidTransaction::with_tag_prefix("DrandOffchainWorker")
                    .priority(priority)
                    .and_provides((b"drand", r).using_encoded(blake2_256))
                    .longevity(3)
                    .propagate(false)
                    .build()
            }

            None => {
                if next_unsigned_at > *block_number {
                    return InvalidTransaction::Stale.into();
                }

                let provides_tag = (next_unsigned_at, public.encode()).using_encoded(blake2_256);
                ValidTransaction::with_tag_prefix("DrandOffchainWorker")
                    .priority(T::UnsignedPriority::get())
                    .and_provides(provides_tag)
                    .longevity(1)
                    .propagate(true)
                    .build()
            }
        }
    }

    fn prune_old_pulses(last_stored_round: RoundNumber) {
        let mut oldest = OldestStoredRound::<T>::get();
        if oldest == 0 {
            return;
        }

        let mut removed: u64 = 0;
        while last_stored_round.saturating_sub(oldest).saturating_add(1) > MAX_KEPT_PULSES
            && removed < MAX_REMOVED_PULSES
        {
            Pulses::<T>::remove(oldest);
            oldest = oldest.saturating_add(1);
            removed = removed.saturating_add(1);
        }

        OldestStoredRound::<T>::put(oldest);
    }
}

/// construct a message (e.g. signed by drand)
pub fn message(current_round: RoundNumber, prev_sig: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::default();
    hasher.update(prev_sig);
    hasher.update(current_round.to_be_bytes());
    hasher.finalize().to_vec()
}

impl<T: Config> Randomness<T::Hash, BlockNumberFor<T>> for Pallet<T> {
    // this function hashes together the subject with the latest known randomness from quicknet
    fn random(subject: &[u8]) -> (T::Hash, BlockNumberFor<T>) {
        let block_number_minus_one =
            <frame_system::Pallet<T>>::block_number().saturating_sub(One::one());

        let last_stored_round = LastStoredRound::<T>::get();

        let mut entropy = T::Hash::default();
        if let Some(pulse) = Pulses::<T>::get(last_stored_round) {
            entropy = (subject, block_number_minus_one, pulse.randomness.clone())
                .using_encoded(T::Hashing::hash);
        }

        (entropy, block_number_minus_one)
    }
}
