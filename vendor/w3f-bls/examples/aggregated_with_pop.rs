#[cfg(feature = "std")]
use sha2::Sha256;
#[cfg(feature = "std")]
use w3f_bls::{
    multi_pop_aggregator::MultiMessageSignatureAggregatorAssumingPoP, schnorr_pop::SchnorrPoP,
    Keypair, Message, ProofOfPossession, ProofOfPossessionGenerator, PublicKey, Signed, ZBLS,
};

/// Run using
/// ```sh
/// cargo run --example aggregated_with_pop
/// ```
fn main() {
    #[cfg(feature = "std")]
    {
        let mut keypairs = [
            Keypair::<ZBLS>::generate(::rand::thread_rng()),
            Keypair::<ZBLS>::generate(::rand::thread_rng()),
        ];
        let msgs = [
            "The ships",
            "hung in the sky",
            "in much the same way",
            "that bricks don’t.",
        ]
        .iter()
        .map(|m| Message::new(b"Some context", m.as_bytes()))
        .collect::<Vec<_>>();
        let sigs = msgs
            .iter()
            .zip(keypairs.iter_mut())
            .map(|(m, k)| k.sign(m))
            .collect::<Vec<_>>();

        let publickeys = keypairs
            .iter()
            .map(|k| k.public.clone())
            .collect::<Vec<_>>();
        let pops = keypairs.iter_mut().map(|k|(ProofOfPossessionGenerator::<ZBLS, Sha256, PublicKey<ZBLS>, SchnorrPoP<ZBLS>>::generate_pok(k))).collect::<Vec<_>>();

        //first make sure public keys have valid pop
        let publickeys = publickeys
            .iter()
            .zip(pops.iter())
            .map(|(publickey, pop)| {
                assert!(ProofOfPossession::<ZBLS, Sha256, PublicKey<ZBLS>>::verify(
                    pop, publickey
                ));
                publickey
            })
            .collect::<Vec<_>>();

        let batch_poped = msgs.iter().zip(publickeys).zip(sigs).fold(
            MultiMessageSignatureAggregatorAssumingPoP::<ZBLS>::new(),
            |mut bpop, ((message, publickey), sig)| {
                bpop.add_message_n_publickey(message, &publickey);
                bpop.add_signature(&sig);
                bpop
            },
        );
        assert!(batch_poped.verify())
    }
}
