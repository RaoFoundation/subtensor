//! Benchmarking setup
#![cfg(feature = "runtime-benchmarks")]
#![allow(clippy::arithmetic_side_effects, clippy::expect_used)]
use super::*;

#[allow(unused)]
use crate::Pallet as Commitments;
use ark_serialize::CanonicalSerialize;
use frame_benchmarking::v2::*;
use frame_support::{assert_ok, traits::ConstU32};
use frame_system::RawOrigin;
use rand_chacha::{ChaCha20Rng, rand_core::SeedableRng};
use sha2::Digest;
use sp_std::vec;
use tle::{ibe::fullident::Identity as TleIdentity, tlock::tle};

use sp_runtime::traits::Bounded;

fn assert_last_event<T: frame_system::pallet::Config>(
    generic_event: <T as frame_system::pallet::Config>::RuntimeEvent,
) {
    frame_system::Pallet::<T>::assert_last_event(generic_event.into());
}

const BENCHMARK_REVEAL_ROUND: u64 = 1000;
const DRAND_QUICKNET_PUBKEY_BYTES: [u8; 96] = [
    131, 207, 15, 40, 150, 173, 238, 126, 184, 181, 240, 31, 202, 211, 145, 34, 18, 196, 55, 224,
    7, 62, 145, 31, 185, 0, 34, 211, 231, 96, 24, 60, 140, 75, 69, 11, 106, 10, 108, 58, 198, 165,
    119, 106, 45, 16, 100, 81, 13, 31, 236, 117, 140, 146, 28, 194, 43, 14, 23, 230, 58, 175, 75,
    203, 94, 214, 99, 4, 222, 156, 248, 9, 189, 39, 76, 167, 59, 171, 74, 245, 166, 233, 199, 106,
    75, 192, 158, 118, 234, 232, 153, 30, 245, 236, 228, 90,
];
const DRAND_QUICKNET_SIGNATURE_BYTES: [u8; 48] = [
    180, 70, 121, 185, 165, 154, 242, 236, 135, 107, 26, 107, 26, 213, 46, 169, 177, 97, 95, 195,
    152, 43, 25, 87, 99, 80, 249, 52, 71, 203, 17, 37, 227, 66, 183, 58, 141, 210, 186, 203, 228,
    126, 75, 107, 99, 237, 94, 57,
];

// This creates an `IdentityInfo` object with `num_fields` extra fields.
// All data is pre-populated with some arbitrary bytes.
fn create_identity_info<T: Config>(_num_fields: u32) -> CommitmentInfo<T::MaxFields> {
    let _data = Data::Raw(
        vec![0; 32]
            .try_into()
            .expect("vec length is less than 64; qed"),
    );

    CommitmentInfo {
        fields: Default::default(),
    }
}

fn produce_benchmark_ciphertext(
    plaintext: &[u8],
    round: u64,
) -> BoundedVec<u8, ConstU32<MAX_TIMELOCK_COMMITMENT_SIZE_BYTES>> {
    let pub_key = <TinyBLS381 as EngineBLS>::PublicKeyGroup::deserialize_compressed(
        &DRAND_QUICKNET_PUBKEY_BYTES[..],
    )
    .expect("benchmark drand public key is valid");

    let msg = {
        let mut hasher = sha2::Sha256::new();
        hasher.update(round.to_be_bytes());
        hasher.finalize().to_vec()
    };
    let identity = TleIdentity::new(b"", vec![msg]);
    let esk = [2u8; 32];
    let rng = ChaCha20Rng::seed_from_u64(0);

    let ciphertext = tle::<TinyBLS381, AESGCMStreamCipherProvider, ChaCha20Rng>(
        pub_key, esk, plaintext, identity, rng,
    )
    .expect("benchmark timelock encryption succeeds");

    let mut ciphertext_bytes = Vec::new();
    ciphertext
        .serialize_compressed(&mut ciphertext_bytes)
        .expect("benchmark timelock ciphertext serializes");

    ciphertext_bytes
        .try_into()
        .expect("benchmark timelock ciphertext fits max size")
}

fn insert_benchmark_pulse<T: Config>(round: u64) {
    let randomness: BoundedVec<u8, ConstU32<32>> = vec![0_u8; 32]
        .try_into()
        .expect("benchmark randomness fits bounded vec");
    let signature: BoundedVec<u8, ConstU32<144>> = DRAND_QUICKNET_SIGNATURE_BYTES
        .to_vec()
        .try_into()
        .expect("benchmark signature fits bounded vec");

    pallet_drand::Pulses::<T>::insert(
        round,
        pallet_drand::types::Pulse {
            round,
            randomness,
            signature,
        },
    );
}

fn timelocked_commitment_info<T: Config>() -> CommitmentInfo<T::MaxFields> {
    let raw = Data::Raw(
        b"timelock benchmark"
            .to_vec()
            .try_into()
            .expect("benchmark raw data fits bounded vec"),
    );
    let inner_fields: BoundedVec<Data, T::MaxFields> =
        BoundedVec::try_from(vec![raw]).expect("benchmark max fields allows raw");
    let inner_info: CommitmentInfo<T::MaxFields> = CommitmentInfo {
        fields: inner_fields,
    };
    let encrypted = produce_benchmark_ciphertext(&inner_info.encode(), BENCHMARK_REVEAL_ROUND);
    let timelock = Data::TimelockEncrypted {
        encrypted,
        reveal_round: BENCHMARK_REVEAL_ROUND,
    };
    let fields =
        BoundedVec::try_from(vec![timelock]).expect("benchmark max fields allows timelock");

    CommitmentInfo { fields }
}

#[benchmarks]
mod benchmarks {
    use super::*;

    #[benchmark]
    fn set_commitment() {
        let netuid = NetUid::from(1);
        let caller: T::AccountId = whitelisted_caller();
        let _ = T::Currency::make_free_balance_be(&caller, BalanceOf::<T>::max_value());

        #[extrinsic_call]
        _(
            RawOrigin::Signed(caller.clone()),
            netuid,
            Box::new(create_identity_info::<T>(0)),
        );

        assert_last_event::<T>(
            Event::<T>::Commitment {
                netuid,
                who: caller,
            }
            .into(),
        );
    }

    #[benchmark]
    fn set_max_space() {
        let new_space: u32 = 1_000;

        #[extrinsic_call]
        _(RawOrigin::Root, new_space);

        assert_eq!(MaxSpace::<T>::get(), new_space);
    }

    #[benchmark(skip_meta)]
    fn reveal_timelocked_commitments() {
        let netuid = NetUid::from(1);
        let caller: T::AccountId = whitelisted_caller();
        let _ = T::Currency::make_free_balance_be(&caller, BalanceOf::<T>::max_value());
        let info = timelocked_commitment_info::<T>();

        assert_ok!(Commitments::<T>::set_commitment(
            RawOrigin::Signed(caller.clone()).into(),
            netuid,
            Box::new(info),
        ));
        insert_benchmark_pulse::<T>(BENCHMARK_REVEAL_ROUND);

        #[block]
        {
            assert_ok!(Commitments::<T>::reveal_timelocked_commitments());
        }

        assert!(RevealedCommitments::<T>::get(netuid, caller).is_some());
    }

    impl_benchmark_test_suite!(Commitments, crate::mock::new_test_ext(), crate::mock::Test);
}
