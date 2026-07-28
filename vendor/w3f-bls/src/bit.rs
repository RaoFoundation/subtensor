//! Aggregate signatures with signers represented by bitfields
//!
//! These tool support both the prefered deliniarized as well as the
//! discuraged proof-of-possession flavors of aggregation.

use core::borrow::{Borrow, BorrowMut};
use core::iter::once;

use ark_ec::Group;
use ark_ff::Zero;

use super::single::SignedMessage;
use super::verifiers::verify_with_distinct_messages;
use super::*;

// Slice equality with bytewise equality hack because
// std does not expose `slice::BytewiseEquality`
fn slice_eq_bytewise<T: PartialEq<T>>(x: &[T], y: &[T]) -> bool {
    if x.len() != y.len() {
        return false;
    }
    if ::core::ptr::eq(x, y) {
        return true;
    }
    x == y
}

/// Signer table required for both delinearization and proofs-of-possession.
///
/// We explicitly provide a signers bitfield type to support fixed sized
/// variants.
pub trait SignerTable<E: EngineBLS> {
    /// Returns true if two `SignerTable` databases match exactly.
    ///
    /// We could employ a PartialEq<Self> + Eq bound for this, except
    /// those are frequently slow even for small for databases.
    fn agreement(&self, other: &Self) -> bool;

    /// Bitfield type used to represent a signers set
    ///
    /// Must not permit altering length, so `Box<[u8]>` or `[u8; 128]` not `Vec<u8>`.
    type Signers: Borrow<[u8]> + BorrowMut<[u8]> + Clone + Sized;
    /// Create a new signers set bitfield
    fn new_signers(&self) -> Self::Signers;

    /// Lookup the public key with a particular bit index.
    ///
    /// Must succeed if `index < signers.borrow().len()`, but
    /// should panic if `index > signers.borrow().len()`.
    /// It may return `None` if the position is empty.
    ///
    /// Must satisfy `self.lookup(i).and_then(|i| self.find(i)) == Some(i)` when `i` is occupied.
    fn lookup(&self, index: usize) -> Option<PublicKey<E>>;

    /// Find the bit index for a particular public key.
    ///
    /// Must succeed if the public key is present, and fail otherwise.
    ///
    /// Must satisfy `self.find(pk).and_then(|i| self.lookup(i)) == Some(pk)` when `pk` is present.
    fn find(&self, publickey: &PublicKey<E>) -> Option<usize>;
}

/// Occupied indices bit mask for `self.signers[offset]`  
fn chunk_lookups<E, ST>(signer_table: &ST, offset: usize) -> u8
where
    E: EngineBLS,
    ST: SignerTable<E>,
{
    (0..8).into_iter().fold(0u8, |b, j| {
        let i = 8 * offset + j;
        let pk = signer_table.lookup(i).filter(|pk| {
            // bb = true always due to check in add_points
            let bb = Some(i) == signer_table.find(&pk);
            debug_assert!(
                bb,
                "Incorrect SignerTable implementation with duplicate publickeys"
            );
            bb
        });
        b | pk.map_or(0u8, |_| 1u8 << j)
    })
}

/// Avoiding duplicate keys inside a slice gets costly.  We suggest
/// improving performance by using a customized data type.
///
/// TODO: Evaluate using Deref vs Borrow in this context
/// TODO: Use specialization here
impl<E, V> SignerTable<E> for V
where
    E: EngineBLS,
    V: ::core::ops::Deref<Target = [PublicKey<E>]>,
{
    fn agreement(&self, other: &Self) -> bool {
        slice_eq_bytewise(self.deref(), other.deref())
    }

    type Signers = Box<[u8]>;
    fn new_signers(&self) -> Self::Signers {
        vec![0u8; (self.deref().len() + 7) / 8].into_boxed_slice()
    }

    fn lookup(&self, index: usize) -> Option<PublicKey<E>> {
        self.deref().get(index).cloned()
        // Checked for duplicates in BitSignedMessage
        // .filter(|publickey| {
        //     Some(index) == self.deref().iter().position(|pk| *pk==publickey);
        //     debug_assert!(b, "Incorrect SignerTable implementation with duplicate publickeys");
        //     b
        // })
    }
    fn find(&self, publickey: &PublicKey<E>) -> Option<usize> {
        self.deref().iter().position(|pk| *pk == *publickey)
        // Checked for duplicates in BitSignedMessage
        // .filter(|index| {
        //     Some(publickey) == self.deref().get(index);
        //     debug_assert!(b, "Incorrect SignerTable implementation with duplicate publickeys");
        //     b
        // })
    }
}

/// Error type for bitfield-style proof-of-possession aggreggation
///
/// These do not necessarily represent attacks pr se.  We therefore
/// permit users to recover from them, although actual recovery sounds
/// impossible nomrally.
#[derive(Debug)]
pub enum SignerTableError {
    /// Attempted to use missmatched proof-of-possession tables.
    BadSignerTable(&'static str),
    /// Attempted to aggregate distint messages, which requires the
    /// the more general SignatureAggregatorAssumingPoP type instead.
    MismatchedMessage,
    /// Aggregation is impossible due to signers being repeated or
    /// repeated too many times in both sets or multi-sets, respectively.
    RepeatedSigners,
}

impl ::core::fmt::Display for SignerTableError {
    fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
        use self::SignerTableError::*;
        match self {
            BadSignerTable(s) => write!(f, "{}", s),
            MismatchedMessage => write!(
                f,
                "Cannot aggregate distinct messages with only a bit field."
            ),
            RepeatedSigners => write!(f, "Cannot aggregate due to duplicate signers."),
        }
    }
}

impl ::std::error::Error for SignerTableError {
    fn description(&self) -> &str {
        use self::SignerTableError::*;
        match self {
            BadSignerTable(s) => s,
            MismatchedMessage => "Cannot aggregate distinct messages with only a bit field.",
            RepeatedSigners => "Cannot aggregate due to duplicate signers",
        }
    }
}

/// One individual message with attached aggreggate BLS signatures
/// from signers for whom we previously checked proofs-of-possession,
/// and with the singers presented as a compact bitfield.
///
/// We may aggregage only one signatures per signer here, but our
/// serialized signature is only one 96 or or 48 bytes compressed
/// curve point, plus the `SignerTable::Signers`, which takes
/// about 1 bit per signer if optimized correctly.
///
/// You must provide a `SignerTable` for this, likely by
/// implementing it for your own data structures.
// #[derive(Clone)]
pub struct BitSignedMessage<E: EngineBLS, POP: SignerTable<E>> {
    proofs_of_possession: POP,
    signers: <POP as SignerTable<E>>::Signers,
    message: Message,
    signature: Signature<E>,
}

impl<'a, E, POP> Clone for BitSignedMessage<E, POP>
where
    E: EngineBLS,
    POP: SignerTable<E> + Clone,
{
    fn clone(&self) -> BitSignedMessage<E, POP> {
        BitSignedMessage {
            proofs_of_possession: self.proofs_of_possession.clone(),
            signers: self.signers.clone(),
            message: self.message.clone(),
            signature: self.signature.clone(),
        }
    }
}

impl<'a, E, POP> Signed for &'a BitSignedMessage<E, POP>
where
    E: EngineBLS,
    POP: SignerTable<E>,
{
    type E = E;

    type M = Message;
    type PKG = PublicKey<E>;

    type PKnM = ::core::iter::Once<(Message, PublicKey<E>)>;

    fn messages_and_publickeys(self) -> Self::PKnM {
        let mut publickey = E::PublicKeyGroup::zero();
        for i in 0..8 * self.signers.borrow().len() {
            if self.signers.borrow()[i / 8] & (1 << (i % 8)) != 0 {
                let pop_pk = self.proofs_of_possession.lookup(i).unwrap();
                if Some(i) != self.proofs_of_possession.find(&pop_pk) {
                    // unreachable due to check in add points
                    debug_assert!(
                        false,
                        "Incorrect SignerTable implementation with duplicate publickeys"
                    );
                    continue;
                }
                publickey += &pop_pk.0;
            }
        }
        once((self.message.clone(), PublicKey(publickey)))
    }

    fn signature(&self) -> Signature<E> {
        self.signature
    }

    fn verify(self) -> bool {
        // We have already aggregated distinct messages, so our distinct
        // message verification code provides reasonable optimizations,
        // except the public keys need not be normalized here.
        // We foresee verification via gaussian elimination being
        // significantly faster, but requiring affine keys.
        verify_with_distinct_messages(self, true)
    }
}

impl<E, POP> BitSignedMessage<E, POP>
where
    E: EngineBLS,
    POP: SignerTable<E>,
{
    pub fn new(proofs_of_possession: POP, message: &Message) -> BitSignedMessage<E, POP> {
        let signers = proofs_of_possession.new_signers();
        let signature = Signature(E::SignatureGroup::zero());
        BitSignedMessage {
            proofs_of_possession,
            signers,
            message: message.clone(),
            signature,
        }
    }

    fn add_points(
        &mut self,
        publickey: PublicKey<E>,
        signature: Signature<E>,
    ) -> Result<(), SignerTableError> {
        let i =
            self.proofs_of_possession
                .find(&publickey)
                .ok_or(SignerTableError::BadSignerTable(
                    "Mismatched proof-of-possession",
                ))?;
        if self.proofs_of_possession.lookup(i) != Some(publickey) {
            return Err(SignerTableError::BadSignerTable(
                "Invalid SignerTable implementation with missmatched lookups",
            ));
        }
        let b = 1 << (i % 8);
        let s = &mut self.signers.borrow_mut()[i / 8];
        if *s & b != 0 {
            return Err(SignerTableError::RepeatedSigners);
        }
        *s |= b;
        self.signature.0 += &signature.0;
        Ok(())
    }

    /// Include one signed message, after testing for message and
    /// proofs-of-possession table agreement, and disjoint publickeys.
    pub fn add(&mut self, signed: &SignedMessage<E>) -> Result<(), SignerTableError> {
        if self.message != signed.message {
            return Err(SignerTableError::MismatchedMessage);
        }
        self.add_points(signed.publickey, signed.signature)
    }

    /// Merge two `BitSignedMessage`, after testing for message
    /// and proofs-of-possession table agreement, and disjoint publickeys.
    pub fn merge(&mut self, other: &BitSignedMessage<E, POP>) -> Result<(), SignerTableError> {
        if self.message != other.message {
            return Err(SignerTableError::MismatchedMessage);
        }
        if !self
            .proofs_of_possession
            .agreement(&other.proofs_of_possession)
        {
            return Err(SignerTableError::BadSignerTable(
                "Mismatched proof-of-possession",
            ));
        }
        for (offset, (x, y)) in self
            .signers
            .borrow()
            .iter()
            .zip(other.signers.borrow())
            .enumerate()
        {
            if *x & *y != 0 {
                return Err(SignerTableError::RepeatedSigners);
            }
            if *y & !chunk_lookups(&self.proofs_of_possession, offset) != 0 {
                return Err(SignerTableError::BadSignerTable("Absent signer"));
            }
        }
        for (x, y) in self
            .signers
            .borrow_mut()
            .iter_mut()
            .zip(other.signers.borrow())
        {
            *x |= y;
        }
        self.signature.0 += &other.signature.0;
        Ok(())
    }
}

/// One individual message with attached aggreggate BLS signatures
/// from signers for whom we previously checked proofs-of-possession,
/// and with the singers presented as a compact bitfield.
///
/// We may aggregage any number of duplicate signatures per signer here,
/// unlike `CountSignedMessage`, but our serialized signature costs
/// one 96 or or 48 bytes compressed curve point, plus the size of
/// `SignerTable::Signers` times log of the largest duplicate count.
///
/// You must provide a `SignerTable` for this, perhapos by implementing
/// it for your own fixed size data structures.
// #[derive(Clone)]
pub struct CountSignedMessage<E: EngineBLS, POP: SignerTable<E>> {
    proofs_of_possession: POP,
    signers: Vec<<POP as SignerTable<E>>::Signers>,
    message: Message,
    signature: Signature<E>,
    /// Errors if a duplicate signature count would exceed this number.  
    /// We suggest rounding up to the nearest power of two.
    pub max_duplicates: usize,
}

impl<'a, E, POP> Clone for CountSignedMessage<E, POP>
where
    E: EngineBLS,
    POP: SignerTable<E> + Clone,
{
    fn clone(&self) -> CountSignedMessage<E, POP> {
        CountSignedMessage {
            proofs_of_possession: self.proofs_of_possession.clone(),
            signers: self.signers.clone(),
            message: self.message.clone(),
            signature: self.signature.clone(),
            max_duplicates: self.max_duplicates,
        }
    }
}

impl<'a, E, POP> Signed for &'a CountSignedMessage<E, POP>
where
    E: EngineBLS,
    POP: SignerTable<E>,
{
    type E = E;

    type M = Message;
    type PKG = PublicKey<E>;

    type PKnM = ::core::iter::Once<(Message, PublicKey<E>)>;

    fn messages_and_publickeys(self) -> Self::PKnM {
        let mut publickey = E::PublicKeyGroup::zero();
        for signers in self.signers.iter().rev().map(|signers| signers.borrow()) {
            publickey.double_in_place();
            for i in 0..8 * signers.len() {
                if signers[i / 8] & (1 << (i % 8)) != 0 {
                    let pop_pk = self.proofs_of_possession.lookup(i).unwrap();
                    if Some(i) != self.proofs_of_possession.find(&pop_pk) {
                        // unreachable due to check in add points
                        debug_assert!(
                            false,
                            "Incorrect SignerTable implementation with duplicate publickeys"
                        );
                        continue;
                    }
                    publickey += &pop_pk.0;
                }
            }
        }
        once((self.message.clone(), PublicKey(publickey)))
    }

    fn signature(&self) -> Signature<E> {
        self.signature
    }

    fn verify(self) -> bool {
        // We have already aggregated distinct messages, so our distinct
        // message verification code provides reasonable optimizations,
        // except the public keys need not be normalized here.
        // We foresee verification via gaussian elimination being
        // significantly faster, but requiring affine keys.
        verify_with_distinct_messages(self, true)
    }
}

impl<E, POP> CountSignedMessage<E, POP>
where
    E: EngineBLS,
    POP: SignerTable<E>,
{
    pub fn new(proofs_of_possession: POP, message: Message) -> CountSignedMessage<E, POP> {
        let signers = vec![proofs_of_possession.new_signers(); 1];
        let signature = Signature(E::SignatureGroup::zero());
        let max_duplicates = 16;
        CountSignedMessage {
            proofs_of_possession,
            signers,
            message,
            signature,
            max_duplicates,
        }
    }

    /*
    fn check_one_lookup(&self, index: usize) -> Result<(),SignerTableError> {
        let e = SignerTableError::BadSignerTable("Invalid SignerTable implementation with missmatched lookups");
        self.proofs_of_possession.lookup(index).filter(|pk| {
            Some(index) == self.proofs_of_possession.find(&pk)
        }).map(|_| ()).ok_or(e)
    }
    */

    fn reserve_depth(&mut self, count: usize) {
        let l = 0usize.leading_zeros() - count.leading_zeros();
        if l as usize <= self.signers.len() {
            return;
        }
        let l = l as usize - self.signers.len();
        self.signers.reserve(l);
        for _i in 0..l {
            self.signers.push(self.proofs_of_possession.new_signers());
        }
    }

    /*
    commented out to rid of unused warning
    TODO: add test coverage to trim and uncomment
    fn trim(&mut self) {
        let empty = |s: &POP::Signers| s.borrow().iter().all(|b| *b == 0u8);
        let c = self.signers.len() - self.signers.iter().rev().take_while(|s| empty(&*s)).count();
        self.signers.truncate(c)
    }*/

    fn test_count(&self, count: usize) -> Result<(), SignerTableError> {
        if count >= self.max_duplicates || count >= usize::max_value() {
            return Err(SignerTableError::RepeatedSigners);
        }
        Ok(())
    }

    /// Get the count of the number of duplicate signatures by the public key with a given index.
    fn get_count(&self, index: usize) -> usize {
        let mut count = 0;
        for signers in self.signers.iter().rev().map(|signers| signers.borrow()) {
            count *= 2;
            if signers[index / 8] & (1 << (index % 8)) != 0 {
                count += 1;
            }
        }
        count
    }

    /// Set the count of the number of duplicate signatures by the public key with a given index.
    /// Always call test_count before invoking this method.
    fn set_count(&mut self, index: usize, mut count: usize) {
        self.reserve_depth(count);
        for signers in self.signers.iter_mut().map(|signers| signers.borrow_mut()) {
            if count & 1usize != 0 {
                signers[index / 8] |= 1 << (index % 8);
            } else {
                signers[index / 8] &= !(1 << (index % 8));
            }
            count /= 2;
        }
    }

    fn add_points(
        &mut self,
        publickey: PublicKey<E>,
        signature: Signature<E>,
    ) -> Result<(), SignerTableError> {
        let i =
            self.proofs_of_possession
                .find(&publickey)
                .ok_or(SignerTableError::BadSignerTable(
                    "Mismatched proof-of-possession",
                ))?;
        if self.proofs_of_possession.lookup(i) != Some(publickey) {
            return Err(SignerTableError::BadSignerTable(
                "Invalid SignerTable implementation with missmatched lookups",
            ));
        }
        let count = self.get_count(i) + 1;
        self.test_count(count)?;
        self.set_count(i, count);
        self.signature.0 += &signature.0;
        Ok(())
    }

    /// Include one signed message, after testing for message and
    /// proofs-of-possession table agreement, and disjoint publickeys.
    pub fn add(&mut self, signed: &SignedMessage<E>) -> Result<(), SignerTableError> {
        if self.message != signed.message {
            return Err(SignerTableError::MismatchedMessage);
        }
        self.add_points(signed.publickey, signed.signature)
    }

    pub fn add_bitsig(&mut self, other: &BitSignedMessage<E, POP>) -> Result<(), SignerTableError> {
        if self.message != other.message {
            return Err(SignerTableError::MismatchedMessage);
        }
        if !self
            .proofs_of_possession
            .agreement(&other.proofs_of_possession)
        {
            return Err(SignerTableError::BadSignerTable(
                "Mismatched proof-of-possession",
            ));
        }
        let os = other.signers.borrow();
        for offset in 0..self.signers[0].borrow().len() {
            if self
                .signers
                .iter()
                .fold(os[offset], |b, s| b | s.borrow()[offset])
                & !chunk_lookups(&self.proofs_of_possession, offset)
                != 0u8
            {
                return Err(SignerTableError::BadSignerTable("Absent signer"));
            }
            for j in 0..8 {
                let mut count = self.get_count(8 * offset + j);
                if os[offset] & (1 << j) != 0 {
                    count += 1;
                }
                self.test_count(count)?;
            }
        }
        for index in 0..8 * self.signers[0].borrow().len() {
            let count = self.get_count(index);
            if os[index / 8] & (1 << (index % 8)) != 0 {
                self.set_count(index, count + 1);
            }
        }
        self.signature.0 += &other.signature.0;
        Ok(())
    }

    /// Merge two `CountSignedMessage`, after testing for message
    /// and proofs-of-possession table agreement, and disjoint publickeys.
    pub fn merge(&mut self, other: &CountSignedMessage<E, POP>) -> Result<(), SignerTableError> {
        if self.message != other.message {
            return Err(SignerTableError::MismatchedMessage);
        }
        if !self
            .proofs_of_possession
            .agreement(&other.proofs_of_possession)
        {
            return Err(SignerTableError::BadSignerTable(
                "Mismatched proof-of-possession",
            ));
        }
        for offset in 0..self.signers[0].borrow().len() {
            if self
                .signers
                .iter()
                .chain(&other.signers)
                .fold(0u8, |b, s| b | s.borrow()[offset])
                & !chunk_lookups(&self.proofs_of_possession, offset)
                != 0u8
            {
                return Err(SignerTableError::BadSignerTable("Absent signer"));
            }
            for j in 0..8 {
                let index = 8 * offset + j;
                self.test_count(self.get_count(index).saturating_add(other.get_count(index)))?;
            }
        }
        for index in 0..8 * self.signers[0].borrow().len() {
            self.set_count(index, self.get_count(index) + other.get_count(index));
        }
        self.signature.0 += &other.signature.0;
        Ok(())
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use rand::thread_rng; // Rng

    use super::*;

    #[test]
    fn proofs_of_possession() {
        let msg1 = Message::new(b"ctx", b"some message");
        let msg2 = Message::new(b"ctx", b"another message");

        let k = |_| Keypair::<ZBLS>::generate(thread_rng());
        let mut keypairs = (0..4).into_iter().map(k).collect::<Vec<_>>();
        let pop = keypairs.iter().map(|k| k.public).collect::<Vec<_>>();
        let dup = keypairs[3].clone();
        keypairs.push(dup);
        let sigs1 = keypairs
            .iter_mut()
            .map(|k| k.signed_message(&msg1))
            .collect::<Vec<_>>();

        let mut bitsig1 = BitSignedMessage::<ZBLS, _>::new(pop.clone(), &msg1);
        assert!(bitsig1.verify()); // verifiers::verify_with_distinct_messages(&dms,true)
        for (i, sig) in sigs1.iter().enumerate().take(2) {
            assert!(bitsig1.add(sig).is_ok() == (i < 4));
            assert!(bitsig1.verify()); // verifiers::verify_with_distinct_messages(&dms,true)
        }
        let mut bitsig1a = BitSignedMessage::<ZBLS, _>::new(pop.clone(), &msg1);
        for (i, sig) in sigs1.iter().enumerate().skip(2) {
            assert!(bitsig1a.add(sig).is_ok() == (i < 4));
            assert!(bitsig1a.verify()); // verifiers::verify_with_distinct_messages(&dms,true)
        }
        assert!(bitsig1.merge(&bitsig1a).is_ok());
        assert!(bitsig1.merge(&bitsig1a).is_err());
        assert!(verifiers::verify_unoptimized(&bitsig1));
        assert!(verifiers::verify_simple(&bitsig1));
        assert!(verifiers::verify_with_distinct_messages(&bitsig1, false));
        // assert!( verifiers::verify_with_gaussian_elimination(&dms) );

        let sigs2 = keypairs
            .iter_mut()
            .map(|k| k.signed_message(&msg2))
            .collect::<Vec<_>>();
        let mut bitsig2 = BitSignedMessage::<ZBLS, _>::new(pop.clone(), &msg2);
        for sig in sigs2.iter().take(3) {
            assert!(bitsig2.add(sig).is_ok());
        }
        assert!(bitsig1.merge(&bitsig2).is_err());

        let mut multimsg =
            multi_pop_aggregator::MultiMessageSignatureAggregatorAssumingPoP::<ZBLS>::new();
        multimsg.aggregate(&bitsig1);
        multimsg.aggregate(&bitsig2);
        assert!(multimsg.verify()); // verifiers::verify_with_distinct_messages(&dms,true)
        assert!(verifiers::verify_unoptimized(&multimsg));
        assert!(verifiers::verify_simple(&multimsg));
        assert!(verifiers::verify_with_distinct_messages(&multimsg, false));

        let oops = Keypair::<ZBLS>::generate(thread_rng()).signed_message(&msg2);
        assert!(bitsig1.add_points(oops.publickey, oops.signature).is_err());
        /*
        TODO: Test that adding signers for an incorrect message fails, but this version angers teh borrow checker.
        let mut oops_pop = pop.clone();
        oops_pop.push(oops.publickey);
        // We should constriuvt a better test here because this only works
        // because pop.len() is not a multiple of 8.
        bitsig1.proofs_of_possession = &oops_pop[..];
        bitsig1.add_points(oops.publickey,oops.signature).unwrap();
        assert!( ! bitsig1.verify() );
        assert!( ! verifiers::verify_unoptimized(&bitsig1) );
        assert!( ! verifiers::verify_simple(&bitsig1) );
        assert!( ! verifiers::verify_with_distinct_messages(&bitsig1,false) );
        */

        let mut countsig = CountSignedMessage::<ZBLS, _>::new(pop.clone(), msg1);
        assert!(countsig.signers.len() == 1);
        assert!(countsig.verify()); // verifiers::verify_with_distinct_messages(&dms,true)
        assert!(countsig.add_bitsig(&bitsig1).is_ok());
        assert!(bitsig1.signature == countsig.signature);
        assert!(countsig.signers.len() == 1);
        assert!(
            bitsig1.messages_and_publickeys().next() == countsig.messages_and_publickeys().next()
        );
        assert!(countsig.verify());
        for (i, sig) in sigs1.iter().enumerate().take(3) {
            assert!(countsig.add(sig).is_ok() == (i < 4));
            assert!(countsig.verify(), "countsig failed at sig {}", i); // verifiers::verify_with_distinct_messages(&dms,true)
        }
        assert!(countsig.add_bitsig(&bitsig1a).is_ok());
        assert!(countsig.add_bitsig(&bitsig1a).is_ok());
        assert!(countsig.add_bitsig(&bitsig2).is_err());
        let countpop2 = countsig.clone();
        assert!(countsig.merge(&countpop2).is_ok());
        assert!(verifiers::verify_unoptimized(&countsig));
        assert!(verifiers::verify_simple(&countsig));
        assert!(verifiers::verify_with_distinct_messages(&countsig, false));
        countsig.max_duplicates = 4;
        assert!(countsig.merge(&countpop2).is_err());
    }
}
