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

//! Benchmarking setup for pallet-drand
use super::*;

#[allow(unused)]
use crate::Pallet as Drand;
use frame_benchmarking::v2::*;
use frame_system::RawOrigin;

pub const DRAND_PULSE: &str = "{\"round\":1000,\"randomness\":\"fe290beca10872ef2fb164d2aa4442de4566183ec51c56ff3cd603d930e54fdd\",\"signature\":\"b44679b9a59af2ec876b1a6b1ad52ea9b1615fc3982b19576350f93447cb1125e342b73a8dd2bacbe47e4b6b63ed5e39\"}";
pub const DRAND_INFO_RESPONSE: &str = "{\"public_key\":\"83cf0f2896adee7eb8b5f01fcad3912212c437e0073e911fb90022d3e760183c8c4b450b6a0a6c3ac6a5776a2d1064510d1fec758c921cc22b0e17e63aaf4bcb5ed66304de9cf809bd274ca73bab4af5a6e9c76a4bc09e76eae8991ef5ece45a\",\"period\":3,\"genesis_time\":1692803367,\"hash\":\"52db9ba70e0cc0f6eaf7803dd07447a1f5477735fd3f661792ba94600c84e971\",\"groupHash\":\"f477d5c89f21a17c863a7f937c6a6d15859414d2be09cd448d4279af331c5d3e\",\"schemeID\":\"bls-unchained-g1-rfc9380\",\"metadata\":{\"beaconID\":\"quicknet\"}}";

#[benchmarks(
     where
         T::Public: From<sp_core::sr25519::Public>,
 )]
mod benchmarks {
    use super::*;

    #[benchmark]
    fn set_beacon_config() {
        let info: BeaconInfoResponse = serde_json::from_str(DRAND_INFO_RESPONSE).unwrap();
        let config = info.try_into_beacon_config().unwrap();

        let alice = sp_keyring::Sr25519Keyring::Alice.public();

        let config_payload = BeaconConfigurationPayload {
            block_number: 1u32.into(),
            config: config.clone(),
            public: alice.into(),
        };

        #[extrinsic_call]
        set_beacon_config(RawOrigin::Root, config_payload.clone(), None);

        assert_eq!(BeaconConfig::<T>::get(), config);
    }

    #[benchmark]
    fn write_pulse() {
        // Deserialize the beacon info and pulse
        let info: BeaconInfoResponse = serde_json::from_str(DRAND_INFO_RESPONSE).unwrap();
        let config = info.try_into_beacon_config().unwrap();
        let u_p: DrandResponseBody = serde_json::from_str(DRAND_PULSE).unwrap();
        let p: Pulse = u_p.try_into_pulse().unwrap();

        let block_number = 1u32.into();
        let alice = sp_keyring::Sr25519Keyring::Alice.public();

        // Set the beacon configuration
        BeaconConfig::<T>::put(config);

        // Create PulsesPayload with a vector of pulses
        let pulses_payload = PulsesPayload {
            block_number,
            pulses: vec![p.clone()], // Wrap the pulse in a vector
            public: alice.into(),
        };

        #[extrinsic_call]
        write_pulse(RawOrigin::Authorized, pulses_payload.clone(), None);

        // Check that the pulse has been stored
        assert_eq!(Pulses::<T>::get(p.round), Some(p));
    }

    #[benchmark]
    fn set_oldest_stored_round() {
        let oldest_stored_round: u64 = 10;

        #[extrinsic_call]
        set_oldest_stored_round(RawOrigin::Root, oldest_stored_round);

        assert_eq!(OldestStoredRound::<T>::get(), oldest_stored_round);
    }

    impl_benchmark_test_suite!(Drand, crate::mock::new_test_ext(), crate::mock::Test);
}
