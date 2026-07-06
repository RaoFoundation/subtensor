use super::*;

use frame_benchmarking::v2::*;
use frame_support::{BoundedVec, pallet_prelude::ConstU32};
use frame_system::RawOrigin;
use sp_core::sr25519;
use sp_std::{vec, vec::Vec};

use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use ml_kem::{
    Ciphertext, KemCore, MlKem768, MlKem768Params,
    kem::{Decapsulate, DecapsulationKey, Encapsulate},
};
use rand_chacha::{ChaCha20Rng, rand_core::SeedableRng};
use stp_shield::ShieldedTransaction;

use codec::Encode;
use sp_consensus_aura::AURA_ENGINE_ID;
use sp_core::crypto::KeyTypeId;
use sp_io::crypto::sr25519_generate;

/// Set Aura authorities directly from sr25519 public keys.
fn set_aura_authorities<T>(pubkeys: &[sr25519::Public])
where
    T: pallet::Config + pallet_aura::Config,
    <T as pallet_aura::Config>::AuthorityId: From<sr25519::Public>,
{
    let auths: BoundedVec<
        <T as pallet_aura::Config>::AuthorityId,
        <T as pallet_aura::Config>::MaxAuthorities,
    > = BoundedVec::truncate_from(pubkeys.iter().map(|pk| (*pk).into()).collect());
    pallet_aura::Authorities::<T>::put(auths);
}

/// Initialize a block with an Aura pre-runtime digest for the given slot.
///
/// Uses `System::initialize` (like real block production) so the digest
/// survives `commit_db()` in the benchmark framework.
fn initialize_block_with_slot<T: frame_system::Config>(slot: u64) {
    let digest = sp_runtime::Digest {
        logs: vec![sp_runtime::DigestItem::PreRuntime(
            AURA_ENGINE_ID,
            slot.encode(),
        )],
    };
    frame_system::Pallet::<T>::initialize(&1u32.into(), &Default::default(), &digest);
}

/// Build a real max-size encrypted ciphertext (8192 bytes wire format).
///
/// Returns `(wire_ciphertext, dec_key)` so the benchmark can measure decryption.
fn build_max_encrypted_payload() -> (Vec<u8>, DecapsulationKey<MlKem768Params>) {
    let mut rng = ChaCha20Rng::from_seed([42u8; 32]);
    let (dec_key, enc_key) = MlKem768::generate(&mut rng);
    let (kem_ct, shared_secret) = enc_key.encapsulate(&mut rng).unwrap();

    // Wire overhead: key_hash(16) + kem_ct_len(2) + kem_ct(1088) + nonce(24) = 1130.
    // Max aead_ct = 8192 − 1130 = 7062.
    // Poly1305 tag = 16 bytes ⇒ max plaintext = 7046.
    let plaintext = vec![0x42u8; 7046];

    let nonce = [0u8; 24];
    let cipher = XChaCha20Poly1305::new(shared_secret.as_slice().into());
    let aead_ct = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: &plaintext,
                aad: &[],
            },
        )
        .expect("AEAD encryption must succeed in benchmark setup");

    let kem_ct_bytes = kem_ct.as_slice();
    let key_hash = [0u8; 16];

    let mut wire = Vec::with_capacity(8192);
    wire.extend_from_slice(&key_hash);
    wire.extend_from_slice(&(kem_ct_bytes.len() as u16).to_le_bytes());
    wire.extend_from_slice(kem_ct_bytes);
    wire.extend_from_slice(&nonce);
    wire.extend_from_slice(&aead_ct);

    debug_assert_eq!(wire.len(), 8192);

    (wire, dec_key)
}

#[benchmarks(
    where
        T: pallet_aura::Config,
        <T as pallet_aura::Config>::AuthorityId: From<sr25519::Public>,
        <T as pallet::Config>::AuthorityId: From<sr25519::Public>,
)]
mod benches {
    use super::*;

    /// Worst-case `announce_next_key`: all 4 rotation steps write storage.
    ///   1. CurrentKey  ← PendingKey  (pre-populated)
    ///   2. PendingKey  ← NextKey     (pre-populated)
    ///   3. NextKey     ← charlie's AuthorKey (next-next author)
    ///   4. AuthorKeys[alice] ← announced key
    #[benchmark]
    fn announce_next_key() {
        let alice = sr25519_generate(KeyTypeId(*b"aura"), Some("//Alice".as_bytes().to_vec()));
        let bob = sr25519_generate(KeyTypeId(*b"aura"), Some("//Bob".as_bytes().to_vec()));
        let charlie = sr25519_generate(KeyTypeId(*b"aura"), Some("//Charlie".as_bytes().to_vec()));

        // Set Aura authorities directly: [alice, bob, charlie].
        set_aura_authorities::<T>(&[alice, bob, charlie]);

        // Initialize block with slot 0 digest via System::initialize.
        // This survives commit_db() unlike deposit_log().
        // Slot 0 → current=alice(0%3), next_next=charlie(2%3).
        initialize_block_with_slot::<T>(0);

        // Pre-populate PendingKey so CurrentKey ← PendingKey writes.
        let old_pending: ShieldEncKey = BoundedVec::truncate_from(vec![0x99; MLKEM768_ENC_KEY_LEN]);
        PendingKey::<T>::put(old_pending.clone());

        // Pre-populate NextKey so PendingKey ← NextKey writes.
        let old_next: ShieldEncKey = BoundedVec::truncate_from(vec![0x77; MLKEM768_ENC_KEY_LEN]);
        NextKey::<T>::put(old_next.clone());

        // Pre-populate AuthorKeys for charlie (next-next) so NextKey gets set.
        let charlie_key: ShieldEncKey = BoundedVec::truncate_from(vec![0x55; MLKEM768_ENC_KEY_LEN]);
        let charlie_id: <T as pallet::Config>::AuthorityId = charlie.into();
        AuthorKeys::<T>::insert(&charlie_id, charlie_key.clone());

        let enc_key: ShieldEncKey = BoundedVec::truncate_from(vec![0x42; MLKEM768_ENC_KEY_LEN]);

        #[extrinsic_call]
        announce_next_key(RawOrigin::None, Some(enc_key.clone()));

        assert_eq!(CurrentKey::<T>::get(), Some(old_pending));
        assert_eq!(PendingKey::<T>::get(), Some(old_next));
        assert_eq!(NextKey::<T>::get(), Some(charlie_key));
        let alice_id: <T as pallet::Config>::AuthorityId = alice.into();
        assert_eq!(AuthorKeys::<T>::get(&alice_id), Some(enc_key));
    }

    /// Worst-case `submit_encrypted`: max-size ciphertext (8192 bytes) with
    /// real ML-KEM-768 + XChaCha20-Poly1305 decryption to account for the
    /// block proposer's off-chain decrypt cost.
    #[benchmark]
    fn submit_encrypted() {
        let who: T::AccountId = whitelisted_caller();

        // Build a real max-size encrypted payload.
        let (wire, dec_key) = build_max_encrypted_payload();
        let ciphertext: BoundedVec<u8, ConstU32<8192>> = BoundedVec::truncate_from(wire);

        #[block]
        {
            // 1. On-chain dispatch (event deposit).
            Pallet::<T>::submit_encrypted(
                RawOrigin::Signed(who.clone()).into(),
                ciphertext.clone(),
            )
            .expect("submit_encrypted dispatch must succeed");

            // 2. Parse wire-format ciphertext (proposer decode).
            let shielded_tx =
                ShieldedTransaction::parse(&ciphertext).expect("wire format must be valid");

            // 3. ML-KEM-768 decapsulate (proposer crypto).
            let ct = Ciphertext::<MlKem768>::try_from(shielded_tx.kem_ct.as_slice())
                .expect("kem_ct must be valid ML-KEM-768 ciphertext");
            let shared_secret = dec_key
                .decapsulate(&ct)
                .expect("decapsulation must succeed");
            let ss: [u8; 32] = shared_secret.into();

            // 4. AEAD decrypt (proposer crypto).
            let aead = XChaCha20Poly1305::new((&ss).into());
            let _plaintext = aead
                .decrypt(
                    XNonce::from_slice(&shielded_tx.nonce),
                    Payload {
                        msg: &shielded_tx.aead_ct,
                        aad: &[],
                    },
                )
                .expect("AEAD decryption must succeed");
        }
    }

    /// Worst-case `store_encrypted`: queue is nearly full (count = limit - 1),
    /// max-size encrypted call data (8192 bytes).
    #[benchmark(extra)]
    fn store_encrypted() {
        let who: T::AccountId = whitelisted_caller();

        // Fill queue to just under the limit for worst-case read path.
        let limit = MaxPendingExtrinsicsLimit::<T>::get();
        let dummy = PendingExtrinsic::<T> {
            who: who.clone(),
            encrypted_call: BoundedVec::truncate_from(vec![0x00; 1]),
            submitted_at: 0u32.into(),
        };
        for i in 0..limit.saturating_sub(1) {
            PendingExtrinsics::<T>::insert(i, dummy.clone());
        }
        NextPendingExtrinsicIndex::<T>::put(limit.saturating_sub(1));

        let encrypted_call: BoundedVec<u8, MaxEncryptedCallSize> =
            BoundedVec::truncate_from(vec![0xAB; 8192]);

        #[extrinsic_call]
        store_encrypted(RawOrigin::Signed(who.clone()), encrypted_call);

        assert_eq!(PendingExtrinsics::<T>::count(), limit);
    }

    /// Benchmark `set_max_pending_extrinsics_number`: root origin, single storage write.
    #[benchmark]
    fn set_max_pending_extrinsics_number() {
        let value: u32 = 500;

        #[extrinsic_call]
        set_max_pending_extrinsics_number(RawOrigin::Root, value);

        assert_eq!(MaxPendingExtrinsicsLimit::<T>::get(), value);
    }

    /// Benchmark `set_on_initialize_weight`: root origin, single storage write.
    /// Uses the maximum allowed value for worst-case.
    #[benchmark]
    fn set_on_initialize_weight() {
        let value: u64 = MAX_ON_INITIALIZE_WEIGHT;

        #[extrinsic_call]
        set_on_initialize_weight(RawOrigin::Root, value);

        assert_eq!(OnInitializeWeight::<T>::get(), value);
    }

    /// Benchmark `set_stored_extrinsic_lifetime`: root origin, single storage write.
    #[benchmark]
    fn set_stored_extrinsic_lifetime() {
        let value: u32 = 100;

        #[extrinsic_call]
        set_stored_extrinsic_lifetime(RawOrigin::Root, value);

        assert_eq!(ExtrinsicLifetime::<T>::get(), value);
    }

    /// Benchmark `set_max_extrinsic_weight`: root origin, single storage write.
    /// Uses the maximum allowed value for worst-case.
    #[benchmark]
    fn set_max_extrinsic_weight() {
        let value: u64 = MAX_ON_INITIALIZE_WEIGHT;

        #[extrinsic_call]
        set_max_extrinsic_weight(RawOrigin::Root, value);

        assert_eq!(MaxExtrinsicWeight::<T>::get(), value);
    }

    impl_benchmark_test_suite!(Pallet, crate::mock::new_test_ext(), crate::mock::Test);
}
