//! Tests for `Pallet::try_unshield_tx` ML-KEM + AEAD decryption.

use crate::mock::*;
use codec::Encode;
use sp_runtime::traits::Block as BlockT;
use stp_shield::{ShieldKeystore, ShieldedTransaction};

use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use ml_kem::{
    EncodedSizeUser, MlKem768Params,
    kem::{Encapsulate, EncapsulationKey},
};
use rand_chacha::{ChaChaRng, rand_core::SeedableRng};
use stc_shield::MemoryShieldKeystore;

#[test]
fn try_unshield_tx_decrypts_extrinsic() {
    let mut rng = ChaChaRng::from_seed([42u8; 32]);
    let keystore = MemoryShieldKeystore::new();

    // Client side: read the announced encapsulation key and encapsulate.
    let pk_bytes = keystore.next_enc_key().unwrap();
    let enc_key =
        EncapsulationKey::<MlKem768Params>::from_bytes(pk_bytes.as_slice().try_into().unwrap());
    let (kem_ct, shared_secret) = enc_key.encapsulate(&mut rng).unwrap();

    // Build the inner extrinsic that we'll encrypt.
    let inner_call = RuntimeCall::System(frame_system::Call::remark {
        remark: vec![1, 2, 3],
    });
    let inner_uxt = <Block as BlockT>::Extrinsic::new_bare(inner_call);
    let plaintext = inner_uxt.encode();

    // AEAD encrypt the extrinsic bytes.
    let nonce = [42u8; 24];
    let cipher = XChaCha20Poly1305::new(shared_secret.as_slice().into());
    let aead_ct = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: &plaintext,
                aad: &[],
            },
        )
        .unwrap();

    // Roll keystore so next -> current (author side).
    keystore.roll_for_next_slot().unwrap();
    let dec_key_bytes = keystore.current_dec_key().unwrap();

    let shielded_tx = ShieldedTransaction {
        key_hash: [0u8; 16],
        kem_ct: kem_ct.as_slice().to_vec(),
        nonce,
        aead_ct,
    };

    let result = crate::Pallet::<Test>::try_unshield_tx::<Block>(dec_key_bytes, shielded_tx);
    assert!(result.is_some());

    let decoded = result.unwrap();
    assert_eq!(decoded.encode(), inner_uxt.encode());
}
