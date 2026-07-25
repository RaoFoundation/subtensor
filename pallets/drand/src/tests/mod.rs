//! Unit tests for `pallet-drand`, split by concept for discoverability.

pub(crate) use crate::{
    BeaconConfig, BeaconConfigurationPayload, BeaconInfoResponse, Call, DrandResponseBody,
    ENDPOINTS, Error, HasMigrationRun, LastStoredRound, MAX_KEPT_PULSES, OldestStoredRound, Pulse,
    Pulses, PulsesPayload, QUICKNET_CHAIN_HASH, migrations::migrate_prune_old_pulses,
    migrations::migrate_set_oldest_round, mock::*,
};
pub(crate) use codec::Encode;
pub(crate) use frame_support::{
    BoundedVec, assert_noop, assert_ok,
    pallet_prelude::{InvalidTransaction, TransactionSource},
    weights::RuntimeDbWeight,
};
pub(crate) use frame_system::RawOrigin;
pub(crate) use sp_core::Get;
pub(crate) use sp_runtime::{
    offchain::{
        OffchainWorkerExt,
        testing::{PendingRequest, TestOffchainExt},
    },
    traits::ValidateUnsigned,
};

/// Round number of the fixture pulse in [`DRAND_PULSE`].
pub const ROUND_NUMBER: u64 = 1000;

/// Canonical Quicknet pulse JSON used across write / fetch tests.
pub const DRAND_PULSE: &str = "{\"round\":1000,\"randomness\":\"fe290beca10872ef2fb164d2aa4442de4566183ec51c56ff3cd603d930e54fdd\",\"signature\":\"b44679b9a59af2ec876b1a6b1ad52ea9b1615fc3982b19576350f93447cb1125e342b73a8dd2bacbe47e4b6b63ed5e39\"}";
/// Canonical Quicknet `/info` JSON for beacon config fixtures.
pub const DRAND_INFO_RESPONSE: &str = "{\"public_key\":\"83cf0f2896adee7eb8b5f01fcad3912212c437e0073e911fb90022d3e760183c8c4b450b6a0a6c3ac6a5776a2d1064510d1fec758c921cc22b0e17e63aaf4bcb5ed66304de9cf809bd274ca73bab4af5a6e9c76a4bc09e76eae8991ef5ece45a\",\"period\":3,\"genesis_time\":1692803367,\"hash\":\"52db9ba70e0cc0f6eaf7803dd07447a1f5477735fd3f661792ba94600c84e971\",\"groupHash\":\"f477d5c89f21a17c863a7f937c6a6d15859414d2be09cd448d4279af331c5d3e\",\"schemeID\":\"bls-unchained-g1-rfc9380\",\"metadata\":{\"beaconID\":\"quicknet\"}}";
/// Malformed JSON used to force decode failures in HTTP endpoint tests.
pub(crate) const INVALID_JSON: &str = r#"{"round":1000,"randomness":"not base64??","signature":}"#;

mod beacon_config;
mod http_fetch;
mod migrations;
mod prune_pulses;
mod validate_unsigned;
mod write_pulse;
