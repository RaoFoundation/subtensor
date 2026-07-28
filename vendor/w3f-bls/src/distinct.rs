//! ## Aggregation for BLS signatures with distinct message.
//!
//! In this module, we provide the linear flavor of aggregate BLS
//! signature in which all messages are required to be distinct.
//! In other words, if all messages are distinct then we cannot add
//! public keys from different pairings anyways.  
//!
//! In verification, we can add different message hashes signed by the
//! same public key, ala `e(g1,s*H(m1)+s*H(m2)) = e(s*g1,H(m1)+H(m2))`,
//! assuming we need not worry about a signers "equivocating" in
//! advance by providing signatures that verify only when aggregated.
//! We cannot exploit this before verification however, due to the
//! requirement to enforce distinct messages.
//!
//! We also note that most signature schemes permit support extremely
//! efficent signer side batching, which normally out performs BLS.
//! It's ocasioanlly worth asking if signers can be trusted to such
//! collected signatures.  See also:
//! - RSA:  https://eprint.iacr.org/2018/082.pdf
//! - Boneh-Boyen:  https://crypto.stanford.edu/~dabo/papers/bbsigs.pdf
//!     http://sci-gems.math.bas.bg:8080/jspui/bitstream/10525/1569/1/sjc096-vol3-num3-2009.pdf

use ark_ff::Zero;
use std::collections::HashMap;

use super::single::SignedMessage;
use super::verifiers::verify_with_distinct_messages;
use super::*;

/// Error tyoe for non-distinct messages found during distinct
/// message aggregation.
///
/// There are numerous scenarios that make recovery from such errors
/// impossible.  We therefore destroy the aggregate signature struct
/// whenever creating this, so that users cannot respond incorrectly
/// to an error message.
#[derive(Debug)]
pub struct AttackViaDuplicateMessages;

impl ::std::fmt::Display for AttackViaDuplicateMessages {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        write!(f, "Attempted to aggregate duplicate messages.")
    }
}

impl ::std::error::Error for AttackViaDuplicateMessages {
    fn description(&self) -> &str {
        "Attempted to aggregate duplicate messages."
    }
}

/// Distinct messages with attached BLS signature
///
/// We can aggregate BLS signatures on distinct messages without
/// additional assuptions or delinearization.  In this variant, there
/// is obviously no aggregation on the signature curve, so verification
/// still requires one pairing per message.  We can however aggregate
/// numerous messages with the same signer, so this works well when
/// a small signer set signs numerous messages, even if the signer set
/// remains unknown.
///
/// We also of course benifit from running one single Miller loop and
/// final exponentiation when compiuting all these pairings.  We note
/// that proofs-of-possession require distinct messages because the
/// message must uniquely single out the signing key, so they may be
/// aggregated or batch verified with distinct message mode, and
/// indeed using distinct messages aggregation is optimal.
///
/// We recommend using this for either batching or aggregation, but
/// we do yet not provide any serialization scheme for the aggregate
/// version.  Instead, you should serialize the aggregated signature
/// seperately, and reconstruct this type using its `add_*` methods.
#[derive(Clone)]
pub struct DistinctMessages<E: EngineBLS> {
    messages_n_publickeys: HashMap<Message, PublicKey<E>>,
    signature: Signature<E>,
}

impl<'a, E: EngineBLS> Signed for &'a DistinctMessages<E> {
    type E = E;

    type M = &'a Message;
    type PKG = &'a PublicKey<Self::E>;
    type PKnM = ::std::collections::hash_map::Iter<'a, Message, PublicKey<E>>;

    fn messages_and_publickeys(self) -> Self::PKnM {
        self.messages_n_publickeys.iter()
    }

    fn signature(&self) -> Signature<E> {
        self.signature
    }

    fn verify(self) -> bool {
        verify_with_distinct_messages(self, false)
    }
}

/*
We do not require an abstract aggregation routine here since only
two quite different types work in this case.
pub trait SignedWithDistinctMessages : Signed {}
impl<E: EngineBLS,M: Message> SignedWithDistinctMessages for SignedMessage<E,M> {}
impl<E: EngineBLS,M: Message> SignedWithDistinctMessages for DistinctMessages<E,M> {}
*/

impl<E: EngineBLS> DistinctMessages<E> {
    pub fn new() -> DistinctMessages<E> {
        DistinctMessages {
            messages_n_publickeys: HashMap::new(),
            signature: Signature(E::SignatureGroup::zero()),
        }
    }

    /// Add only a `Signature<E>` to our internal signature.
    ///
    /// Useful in constructing an aggregate signature from this type.
    pub fn add_signature(&mut self, signature: &Signature<E>) {
        self.signature.0 += &signature.0;
    }

    /// Add only a `Message` and `PublicKey<E>` to our internal data.
    ///
    /// Useful in constructing an aggregate signature from this type.
    ///
    /// We require that duplicate message halt verification by consuming
    /// self by vaule and return it only if no duplicates occur.
    pub fn add_message_n_publickey(
        mut self,
        message: Message,
        publickey: PublicKey<E>,
    ) -> DistinctMessagesResult<E> {
        if let Some(_old_publickey) = self.messages_n_publickeys.insert(message, publickey) {
            // We need not recover from this error because the hash map gets erased.
            // self.messages_n_publickeys.insert(signed.message,old_publickey);
            return Err(AttackViaDuplicateMessages);
        }
        Ok(self)
    }

    /// Aggregage BLS signatures from singletons with distinct messages
    ///
    /// We require that duplicate message halt verification by consuming
    /// self by vaule and return it only if no duplicates occur.
    pub fn add(self, signed: &SignedMessage<E>) -> DistinctMessagesResult<E> {
        let mut me = self.add_message_n_publickey(signed.message.clone(), signed.publickey)?;
        me.add_signature(&signed.signature);
        Ok(me)
    }

    /// Aggregage BLS signatures from sources with distinct messages
    ///
    /// We require that duplicate message halt verification by consuming
    /// self by vaule and return it only if no duplicates occur.
    pub fn merge(mut self, signed: &DistinctMessages<E>) -> DistinctMessagesResult<E> {
        // We need not detect duplicates early for recovery because
        // duplicates cause our hashmap to be freed anyways.
        // for (m,_pk) in signed.messages_n_publickeys.iter() {
        //     if self.messages_n_publickeys.contains_key(m) {
        //      return Err(AttackViaDuplicateMessages);
        //     }
        // }
        for (m, pk) in signed.messages_n_publickeys.iter() {
            // assert!(self.messages_n_publickeys.insert(*m,*pk).is_none());
            if self.messages_n_publickeys.insert(m.clone(), *pk).is_some() {
                return Err(AttackViaDuplicateMessages);
            }
        }
        self.add_signature(&signed.signature);
        Ok(self)
    }
}

pub type DistinctMessagesResult<E> = Result<DistinctMessages<E>, AttackViaDuplicateMessages>;

/*
TODO: Adopt .collect::<DistinctMessagesResult<E>>() via FromIterator
      whenever https://github.com/rust-lang/rfcs/issues/1856 gets resolved.
impl<'a,E: EngineBLS> FromIterator<&'a SignedMessage<E>> for DistinctMessagesResult<E> {
    fn from_iter<II>(ii: II) -> Self
    where II: IntoIterator<Item = &'a SignedMessage<E>>,
    {
        ii.into_iter().try_fold(DistinctMessages::<ZBLS>::new(), |dm,sm| dm.add(sm))
    }
}
*/

#[cfg(all(test, feature = "std"))]
mod tests {
    use rand::thread_rng; // Rng

    use super::*;

    #[test]
    fn distinct_messages() {
        let msgs = [
            Message::new(b"ctx", b"Message1"),
            Message::new(b"ctx", b"Message1"),
            Message::new(b"ctx", b"Message2"),
            Message::new(b"ctx", b"Message3"),
            Message::new(b"ctx", b"Message4"),
        ];

        let k = |_| Keypair::<ZBLS>::generate(thread_rng());
        let mut keypairs = (0..4).into_iter().map(k).collect::<Vec<_>>();
        let dup = keypairs[3].clone();
        keypairs.push(dup);

        let sigs = msgs
            .iter()
            .zip(keypairs.iter_mut())
            .map(|(m, k)| k.signed_message(m))
            .collect::<Vec<_>>();

        let dm_new = || DistinctMessages::<ZBLS>::new();
        fn dm_add(
            dm: DistinctMessages<ZBLS>,
            sig: &SignedMessage<ZBLS>,
        ) -> Result<DistinctMessages<ZBLS>, AttackViaDuplicateMessages> {
            dm.add(sig)
        }

        let mut dms = sigs.iter().skip(1).try_fold(dm_new(), dm_add).unwrap();
        assert!(dms.messages_and_publickeys().len() == 4);
        let dms0 = sigs.iter().skip(1).try_fold(dm_new(), dm_add).unwrap();
        assert!(dms0.merge(&dms).is_err());
        assert!(sigs.iter().try_fold(dm_new(), dm_add).is_err());
        assert!(dms.verify()); // verifiers::verify_with_distinct_messages(&dms,false)
        assert!(verifiers::verify_unoptimized(&dms));
        assert!(verifiers::verify_simple(&dms));
        assert!(verifiers::verify_with_distinct_messages(&dms, true));
        // assert!( verifiers::verify_with_gaussian_elimination(&dms) );

        let dms1 = sigs
            .iter()
            .skip(1)
            .take(2)
            .try_fold(dm_new(), dm_add)
            .unwrap();
        let dms2 = sigs.iter().skip(3).try_fold(dm_new(), dm_add).unwrap();
        assert!(dms1.merge(&dms2).unwrap().signature == dms.signature);

        *(dms.messages_n_publickeys.get_mut(&msgs[1]).unwrap()) = keypairs[0].public.clone();
        assert!(!dms.verify(), "Verification by an incorrect signer passed");
    }
}
