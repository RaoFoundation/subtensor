#[cfg(feature = "std")]
use w3f_bls::{Keypair, Message, ZBLS};

/// Run using
/// ```sh
/// cargo run --example simple
/// ```
fn main() {
    #[cfg(feature = "std")]
    {
        let mut keypair = Keypair::<ZBLS>::generate(::rand::thread_rng());
        let message = Message::new(b"Some context", b"Some message");
        let sig = keypair.sign(&message);
        assert!(sig.verify(&message, &keypair.public));
    }
}
