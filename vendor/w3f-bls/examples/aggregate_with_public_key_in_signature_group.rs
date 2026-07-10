#[cfg(feature = "std")]
use sha2::Sha256;
#[cfg(feature = "std")]
use w3f_bls::{
    single_pop_aggregator::SignatureAggregatorAssumingPoP, DoublePublicKeyScheme, EngineBLS,
    Keypair, Message, PublicKey, PublicKeyInSignatureGroup, Signed, TinyBLS, TinyBLS377,
};

#[cfg(feature = "std")]
use ark_bls12_377::Bls12_377;
#[cfg(feature = "std")]
use ark_ff::Zero;
#[cfg(feature = "std")]
use rand::thread_rng;

/// Run using
/// ```sh
/// cargo run --example aggregated_with_public_key_in_signature_group.rs
/// ```
fn main() {
    #[cfg(feature = "std")]
    {
        let message = Message::new(b"ctx", b"I'd far rather be happy than right any day.");
        let mut keypairs: Vec<_> = (0..3)
            .into_iter()
            .map(|_| Keypair::<TinyBLS<Bls12_377, ark_bls12_377::Config>>::generate(thread_rng()))
            .collect();
        let pub_keys_in_sig_grp: Vec<PublicKeyInSignatureGroup<TinyBLS377>> = keypairs
            .iter()
            .map(|k| k.into_public_key_in_signature_group())
            .collect();
        let mut prover_aggregator =
            SignatureAggregatorAssumingPoP::<TinyBLS377>::new(message.clone());
        let mut aggregated_public_key =
            PublicKey::<TinyBLS377>(<TinyBLS377 as EngineBLS>::PublicKeyGroup::zero());

        //sign and aggegate
        keypairs
            .iter_mut()
            .for_each(|k| {
                prover_aggregator.add_signature(&k.sign(&message));
                aggregated_public_key.0 += k.public.0;
            });
        
        let mut verifier_aggregator = SignatureAggregatorAssumingPoP::<TinyBLS377>::new(message);
	//get the signature and already aggregated public key from the prover
        verifier_aggregator.add_signature(&(&prover_aggregator).signature());
        verifier_aggregator.add_publickey(&aggregated_public_key);

        //aggregate public keys in signature group
        pub_keys_in_sig_grp.iter().for_each(|pk| {verifier_aggregator.add_auxiliary_public_key(pk);});

        assert!(
            verifier_aggregator.verify_using_aggregated_auxiliary_public_keys::<Sha256>(),
            "verifying with honest auxilary public key should pass"
        );
    }
}
