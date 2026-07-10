//! ## Unaggreagated BLS signatures
//!
//! We simplify the code by using only the projective form as
//! produced by algebraic operations, like aggregation, signing, and
//! `SecretKey::into_public`, for both `Signature` and `Group`.
//!
//! In principle, one benifits from an affine form in serialization,
//! and pairings meaning signature verification, but the conversion
//! from affine to projective is always free and the converion from
//! projective to affine is free if we do no algebraic operations.  
//! We thus expect the conversion to and from projective to be free
//! in the case of verifications where staying affine yields the
//! largest benifits.
//!
//! We imagine this simplification helps focus on more important
//! optimizations, like placing `batch_normalization` calls well.
//! We could exploit `CurveGroup: += _mixed` function
//! if we had seperate types for affine points, but if doing so
//! improved performance enough then we instead suggest tweaking
//! `CurveGroup::add_mixed` to test for normalized points.
//!
//! TODO: Add serde support for serialization throughout.  See
//!  https://github.com/ebfull/pairing/pull/87#issuecomment-402397091
//!  https://github.com/poanetwork/hbbft/blob/38178af1244ddeca27f9d23750ca755af6e886ee/src/crypto/serde_impl.rs#L95

use alloc::{vec, vec::Vec};

use ark_ff::field_hashers::{DefaultFieldHasher, HashToField};
use ark_ff::{UniformRand, Zero};

use ark_ec::{AffineRepr, CurveGroup};

use ark_serialize::{
    CanonicalDeserialize, CanonicalSerialize, Compress, Read, SerializationError, Valid, Validate,
    Write,
};
#[cfg(feature = "std")]
use rand::thread_rng;
use rand::{rngs::StdRng, Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use sha2::Sha256;
use sha3::{
    digest::{ExtendableOutput, Update, XofReader},
    Shake128,
};

use digest::Digest;

use core::iter::once;

use crate::serialize::SerializableToBytes;
use crate::{EngineBLS, Message, Signed};
// //////////////// SECRETS //////////////// //

/// Secret signing key lacking the side channel protections from
/// key splitting.  Avoid using directly in production.
#[derive(CanonicalSerialize, CanonicalDeserialize)]
pub struct SecretKeyVT<E: EngineBLS>(pub E::Scalar);

impl<E: EngineBLS> Clone for SecretKeyVT<E> {
    fn clone(&self) -> Self {
        SecretKeyVT(self.0)
    }
}

impl<E: EngineBLS> SecretKeyVT<E> {
    /// Generate a secret key without side channel protections.
    pub fn generate<R: Rng>(mut rng: R) -> Self {
        SecretKeyVT(E::generate(&mut rng))
    }

    pub fn from_seed(seed: &[u8]) -> Self {
        let hasher = <DefaultFieldHasher<Sha256> as HashToField<E::Scalar>>::new(&[]);
        return SecretKeyVT(hasher.hash_to_field(seed, 1)[0]);
    }
}

impl<E: EngineBLS> SecretKeyVT<E> {
    /// Sign without side channel protections from key mutation.
    pub fn sign(&self, message: &Message) -> Signature<E> {
        let mut s: E::SignatureGroup = message.hash_to_signature_curve::<E>();
        s *= self.0;
        // s.normalize();   // VRFs are faster if we only normalize once, but no normalize method exists.
        // E::SignatureGroup::batch_normalization(&mut [&mut s]);
        Signature(s)
    }

    /// Convert into a `SecretKey` that supports side channel protections,
    /// but does not itself resplit the key.
    pub fn into_split_dirty(&self) -> SecretKey<E> {
        SecretKey {
            key: [self.0.clone(), E::Scalar::zero()],
            old_unsigned: E::SignatureGroup::zero(),
            old_signed: E::SignatureGroup::zero(),
        }
    }

    /// Convert into a `SecretKey` applying side channel protections.
    pub fn into_split<R: Rng>(&self, mut rng: R) -> SecretKey<E> {
        let mut s = self.into_split_dirty();
        s.resplit(&mut rng);
        s.init_point_mutation(rng);
        s
    }

    /// Derive our public key from our secret key
    pub fn into_public(&self) -> PublicKey<E> {
        // TODO str4d never decided on projective vs affine here, so benchmark both versions.
        PublicKey(<E::PublicKeyGroup as CurveGroup>::Affine::generator().into_group() * self.0)
        // let mut g = <E::PublicKeyGroup as CurveGroup>::one();
        // g *= self.0;
        // PublicKey(p)
    }
}

/// Secret signing key that is split to provide side channel protection.
///
/// A simple key splitting works because
/// `self.key[0] * H(message) + self.key[1] * H(message) = (self.key[0] + self.key[1]) * H(message)`.
/// In our case, we mutate the point being signed too by keeping
/// an old point in both signed and unsigned forms, so our message
/// point becomes `new_unsigned = H(message) - old_unsigned`,
/// we compute `new_signed = self.key[0] * new_unsigned + self.key[1] * new_unsigned`,
/// and our signature becomes `new_signed + old_signed`.
/// We save the new signed and unsigned values as old ones, so that adversaries
/// also cannot know the curves points being multiplied by scalars.
/// In this, our `init_point_mutation` method signs some random point,
/// so that even an adversary who tracks all signed messages cannot
/// foresee the curve points being signed.
///
#[cfg_attr(
    feature = "std",
    doc = r##"
/// We require mutable access to the secret key, but interior mutability
/// can easily be employed, which might resemble:
/// ```rust,no_run
/// # extern crate bls_like as bls;
/// # extern crate rand;
/// # use bls::{SecretKey,ZBLS,Message};
/// # #[cfg(feature=std)]
/// # use rand::thread_rng;
/// # let message = Message::new(b"ctx",b"test message");
/// let mut secret = ::std::cell::RefCell::new(SecretKey::<ZBLS>::generate(thread_rng()));
/// let signature = secret.borrow_mut().sign(message,thread_rng());
/// ```
/// If however `secret: Mutex<SecretKey>` or `secret: RwLock<SecretKey>`
/// then one might avoid holding the write lock while signing, or even
/// while sampling the random numbers by using other methods.
"##
)]
///
/// Right now, we serialize using `SecretKey::into_vartime` and
/// `SecretKeyVT::write`, so `secret.into_vartime().write(writer)?`.
/// We deserialize using the `read`, `from_repr`, and `into_split`
/// methods of `SecretKeyVT`, so roughly
/// `SecretKeyVT::from_repr(SecretKeyVT::read(reader) ?) ?.into_split(thread_rng())`.
///
/// TODO: Provide sensible `to_bytes` and `from_bytes` methods
/// for `ZBLS` and `TinyBLS<..>`.
///
/// TODO: Is Pippenger’s algorithm, or another fast MSM algorithm,
/// secure when used with key splitting?

/// Secret signing key including the side channel protections from
/// key splitting.
pub struct SecretKey<E: EngineBLS> {
    key: [E::Scalar; 2],
    old_unsigned: E::SignatureGroup,
    old_signed: E::SignatureGroup,
}

impl<E: EngineBLS> Clone for SecretKey<E> {
    fn clone(&self) -> Self {
        SecretKey {
            key: self.key.clone(),
            old_unsigned: self.old_unsigned.clone(),
            old_signed: self.old_signed.clone(),
        }
    }
}

impl<E: EngineBLS> SecretKey<E>
where
    E: EngineBLS,
{
    /// Generate a secret key that is already split for side channel protection,
    /// but does not apply signed point mutation.
    pub fn generate_dirty<R: Rng>(mut rng: R) -> Self {
        SecretKey {
            key: [E::generate(&mut rng), E::generate(&mut rng)],
            old_unsigned: E::SignatureGroup::zero(),
            old_signed: E::SignatureGroup::zero(),
        }
    }

    /// Generate a secret key that is already split for side channel protection.
    pub fn generate<R: Rng>(mut rng: R) -> Self {
        let mut s = Self::generate_dirty(&mut rng);
        s.init_point_mutation(rng);
        s
    }

    pub fn from_seed(seed: &[u8]) -> Self {
        SecretKeyVT::from_seed(seed).into_split_dirty()
    }
}

impl<E: EngineBLS> SecretKey<E> {
    /// Initialize the signature curve signed point mutation.
    ///
    /// Amortized over many signings involing this once costs
    /// nothing, but each individual invokation costs as much
    /// as signing.
    pub fn init_point_mutation<R: Rng>(&mut self, mut rng: R) {
        let mut s = <E::SignatureGroup as UniformRand>::rand(&mut rng);
        self.old_unsigned = s;
        self.old_signed = s;
        self.old_signed *= self.key[0];
        s *= self.key[1];
        self.old_signed += &s;
    }

    /// Create a representative usable for operations lacking
    /// side channel protections.  
    pub fn into_vartime(&self) -> SecretKeyVT<E> {
        let mut secret = self.key[0].clone();
        secret += &self.key[1];
        SecretKeyVT(secret)
    }

    /// Randomly adjust how we split our secret signing key.
    //
    // An initial call to this function after deserialization or
    // `into_split_dirty` incurs a miniscule risk from side channel
    // attacks, but then protects the highly vulnerable signing
    // operations.  `into_split` itself handles this.
    #[inline(never)]
    pub fn resplit<R: Rng>(&mut self, mut rng: R) {
        // resplit_with(|| Ok(self), rng).unwrap();
        let x = E::generate(&mut rng);
        self.key[0] += &x;
        self.key[1] -= &x;
    }

    /// Sign without doing the key resplit mutation that provides side channel protection.
    ///
    /// Avoid using directly without appropriate `replit` calls, but maybe
    /// useful in proof-of-concenpt code, as it does not require a mutable
    /// secret key.
    pub fn sign_once(&mut self, message: &Message) -> Signature<E> {
        let mut z = message.hash_to_signature_curve::<E>();
        z -= &self.old_unsigned;
        self.old_unsigned = z.clone();
        let mut t = z.clone();
        t *= self.key[0];
        z *= self.key[1];
        z += &t;
        let old_signed = self.old_signed.clone();
        self.old_signed = z.clone();
        z += &old_signed;
        // s.normalize();   // VRFs are faster if we only normalize once, but no normalize method exists.
        // E::SignatureGroup::batch_normalization(&mut [&mut s]);
        Signature(z)
    }

    /// Sign after respliting the secret key for side channel protections.
    pub fn sign<R: Rng>(&mut self, message: &Message, rng: R) -> Signature<E> {
        self.resplit(rng);
        self.sign_once(message)
    }

    /// Derive our public key from our secret key
    ///
    /// We do not resplit for side channel protections here since
    /// this call should be rare.
    pub fn into_public(&self) -> PublicKey<E> {
        let generator = <E::PublicKeyGroup as CurveGroup>::Affine::generator();
        let mut publickey = generator * self.key[0];
        publickey += generator.into_group() * self.key[1];
        PublicKey(publickey)
        // TODO str4d never decided on projective vs affine here, so benchmark this.
        /*
        let mut x = <E::PublicKeyGroup as CurveGroup>::one();
        x *= self.0;
        let y = <E::PublicKeyGroup as CurveGroup>::one();
        y *= self.1;
        x += &y;
        PublicKey(x)
        */
    }
}

// ////////////// NON-SECRETS ////////////// //

// /////// BEGIN MACROS /////// //

/*
TODO: Requires specilizatin
macro_rules! borrow_wrapper {
    ($wrapper:tt,$wrapped:tt,$var:tt) => {
impl<E: EngineBLS> Borrow<E::$wrapped> for $wrapper<E> {
    borrow(&self) -> &E::$wrapped { &self.$var }
}
impl<E: EngineBLS> BorrowMut<E::$wrapped> for $wrapper<E> {
    borrow_mut(&self) -> &E::$wrapped { &self.$var }
}
    }
} // macro_rules!
*/

#[macro_export]
macro_rules! broken_derives {
    ($wrapper:tt) => {
        impl<E: EngineBLS> Clone for $wrapper<E> {
            fn clone(&self) -> Self {
                $wrapper(self.0)
            }
        }
        impl<E: EngineBLS> Copy for $wrapper<E> {}

        impl<E: EngineBLS> PartialEq<Self> for $wrapper<E> {
            fn eq(&self, other: &Self) -> bool {
                self.0.eq(&other.0)
            }
        }

        impl<E: EngineBLS> Eq for $wrapper<E> {}
    };
} // macro_rules!

// //////// END MACROS //////// //

/// Implementing de/serialization for secret keypair
/// Note that deriving serialization for secret is not sensible
/// as you need to conver them to vartime form first
impl<E> CanonicalSerialize for SecretKey<E>
where
    E: EngineBLS,
{
    #[inline]
    fn serialize_with_mode<W: Write>(
        &self,
        writer: W,
        compress: ark_serialize::Compress,
    ) -> Result<(), SerializationError> {
        self.into_vartime().serialize_with_mode(writer, compress)
    }

    #[inline]
    fn serialize_compressed<W: Write>(&self, writer: W) -> Result<(), SerializationError> {
        self.into_vartime().serialize_compressed(writer)?;
        Ok(())
    }

    #[inline]
    fn serialized_size(&self, compress: Compress) -> usize {
        self.into_vartime().serialized_size(compress)
    }

    #[inline]
    fn serialize_uncompressed<W: Write>(&self, mut writer: W) -> Result<(), SerializationError> {
        self.into_vartime().serialize_uncompressed(&mut writer)?;
        Ok(())
    }

    #[inline]
    fn uncompressed_size(&self) -> usize {
        self.into_vartime().uncompressed_size()
    }

    // #[inline]
    // fn serialize_uncompressed_<W: Write>(&self, mut writer: W) -> Result<(), SerializationError> {
    //     self.into_vartime().uncompressed_size().serialize_unchecked(&mut writer)?;
    //     Ok(())
    // }
}

impl<E> Valid for SecretKey<E>
where
    E: EngineBLS,
{
    fn check(&self) -> Result<(), SerializationError> {
        //TODO probabaly turn into vartime and check that because vartime impl valid
        match (self.key[1].check(), self.key[2].check()) {
            (Ok(()), Ok(())) => Ok(()),
            _ => Err(SerializationError::InvalidData),
        }
    }
}

impl<E> CanonicalDeserialize for SecretKey<E>
where
    E: EngineBLS,
{
    fn deserialize_with_mode<R: Read>(
        reader: R,
        compress: Compress,
        validate: Validate,
    ) -> Result<Self, SerializationError> {
        let secret_key_vt = <SecretKeyVT<E> as CanonicalDeserialize>::deserialize_with_mode(
            reader, compress, validate,
        )?;
        Ok(secret_key_vt.into_split_dirty())
    }

    #[inline]
    fn deserialize_compressed<R: Read>(reader: R) -> Result<Self, SerializationError> {
        let secret_key_vt =
            <SecretKeyVT<E> as CanonicalDeserialize>::deserialize_compressed(reader)?;
        Ok(secret_key_vt.into_split_dirty())
    }

    #[inline]
    fn deserialize_uncompressed<R: Read>(mut reader: R) -> Result<Self, SerializationError> {
        let secret_key_vt =
            <SecretKeyVT<E> as CanonicalDeserialize>::deserialize_uncompressed(&mut reader)?;
        Ok(secret_key_vt.into_split_dirty())
    }

    #[inline]
    fn deserialize_uncompressed_unchecked<R: Read>(reader: R) -> Result<Self, SerializationError> {
        let secret_key_vt =
            <SecretKeyVT<E> as CanonicalDeserialize>::deserialize_uncompressed_unchecked(reader)?;
        Ok(secret_key_vt.into_split_dirty())
    }
}

//TODO: when const generic becomes stable we get the size from the trait and return
//      constant size array so it can be implemented as follows
// impl <E: EngineBLS> SerializableToBytes<{ E::SIGNATURE_SERIALIZED_SIZE }> for Signature<E> {}
// impl <E: EngineBLS> SerializableToBytes<{ PublicKey::E::PUBLICKEY_SERIALIZED_SIZE }> for PublicKey<E>  {}
impl<E: EngineBLS> SerializableToBytes for Signature<E> {
    const SERIALIZED_BYTES_SIZE: usize = E::SIGNATURE_SERIALIZED_SIZE;
}
impl<E: EngineBLS> SerializableToBytes for PublicKey<E> {
    const SERIALIZED_BYTES_SIZE: usize = E::PUBLICKEY_SERIALIZED_SIZE;
}
impl<E: EngineBLS> SerializableToBytes for SecretKeyVT<E> {
    const SERIALIZED_BYTES_SIZE: usize = E::SECRET_KEY_SIZE;
}

impl<E: EngineBLS> SerializableToBytes for SecretKey<E> {
    const SERIALIZED_BYTES_SIZE: usize = E::SECRET_KEY_SIZE;
}

/// because SecretKey is not canonically serializable and that we need to convert
/// it to vartime first we need to manually re-implement this trait for secret keys
//, CanonicalSerialize, CanonicalDeserialize)]
/// Detached BLS Signature
#[derive(Debug, CanonicalSerialize, CanonicalDeserialize)]
pub struct Signature<E: EngineBLS>(pub E::SignatureGroup);
// TODO: Serialization

broken_derives!(Signature); // Actually the derive works for this one, not sure why.

impl<E: EngineBLS> Signature<E> {
    //const DESCRIPTION : &'static str = "A BLS signature";

    /// Verify a single BLS signature
    pub fn verify(&self, message: &Message, publickey: &PublicKey<E>) -> bool {
        let publickey = E::prepare_public_key(publickey.0);
        // TODO: Bentchmark these two variants
        // Variant 1.  Do not batch any normalizations
        let message = E::prepare_signature(message.hash_to_signature_curve::<E>());
        let signature = E::prepare_signature(self.0);
        // Variant 2.  Batch signature curve normalizations
        //   let mut s = [E::hash_to_signature_curve(message), signature.0];
        //   E::SignatureCurve::batch_normalization(&s);
        //   let message = s[0].into_affine().prepare();
        //   let signature = s[1].into_affine().prepare();
        // TODO: Compare benchmarks on variants
        E::verify_prepared(signature, &[(publickey, message)])
    }
}

/// BLS Public Key
#[derive(Debug, CanonicalSerialize, CanonicalDeserialize)]
pub struct PublicKey<E: EngineBLS>(pub E::PublicKeyGroup);
// TODO: Serialization

// impl<E: EngineBLS> PublicKey<E> where E: DeserializePublicKey {
//     pub fn i_have_checked_this_proof_of_possession(self) -> PublicKey<PoP<E>> {
//         PublicKey(self.0)
//     }
// }

broken_derives!(PublicKey);
//serialization!(PublicKey,PublicKeyGroup,EngineBLS,EngineBLS);

impl<E: EngineBLS> PublicKey<E> {
    //const DESCRIPTION : &'static str = "A BLS signature";
    pub fn verify(&self, message: &Message, signature: &Signature<E>) -> bool {
        signature.verify(&message, self)
    }
}

/// BLS Keypair
///
/// We create `Signed` messages with a `Keypair` to avoid recomputing
/// the public key, which usually takes longer than signing when
/// the public key group is `G2`.2
///
/// We provide constant-time signing using key splitting.
pub struct KeypairVT<E: EngineBLS> {
    pub secret: SecretKeyVT<E>,
    pub public: PublicKey<E>,
}

impl<E: EngineBLS> Clone for KeypairVT<E> {
    fn clone(&self) -> Self {
        KeypairVT {
            secret: self.secret.clone(),
            public: self.public.clone(),
        }
    }
}

// TODO: Serialization
impl<E: EngineBLS> KeypairVT<E> {
    /// Generate a `Keypair`
    pub fn generate<R: Rng>(rng: R) -> Self {
        let secret = SecretKeyVT::generate(rng);
        let public = secret.into_public();
        KeypairVT { secret, public }
    }
}

impl<E: EngineBLS> KeypairVT<E> {
    /// Convert into a `SecretKey` applying side channel protections.
    pub fn into_split<R: Rng>(&self, rng: R) -> Keypair<E> {
        let secret = self.secret.into_split(rng);
        let public = self.public;
        Keypair { secret, public }
    }

    /// Sign a message creating a `SignedMessage` using a user supplied CSPRNG for the key splitting.
    pub fn sign(&self, message: &Message) -> Signature<E> {
        self.secret.sign(message)
    }

    /// Sign a message creating a `SignedMessage` using a user supplied CSPRNG for the key splitting.
    pub fn signed_message(&self, message: &Message) -> SignedMessage<E> {
        let signature = self.secret.sign(&message);
        SignedMessage {
            message: message.clone(),
            publickey: self.public.clone(),
            signature,
        }
    }
}

/// BLS Keypair
///
/// We create `Signed` messages with a `Keypair` to avoid recomputing
/// the public key, which usually takes longer than signing when
/// the public key group is `G2`.
///
/// We provide constant-time signing using key splitting.
pub struct Keypair<E: EngineBLS> {
    pub secret: SecretKey<E>,
    pub public: PublicKey<E>,
}

impl<E: EngineBLS> Clone for Keypair<E> {
    fn clone(&self) -> Self {
        Keypair {
            secret: self.secret.clone(),
            public: self.public.clone(),
        }
    }
}

// TODO: Serialization
impl<E: EngineBLS> Keypair<E> {
    /// Generate a `Keypair`
    pub fn generate<R: Rng>(rng: R) -> Self {
        let secret = SecretKey::generate(rng);
        let public = secret.into_public();
        Keypair { secret, public }
    }
}

impl<E: EngineBLS> Keypair<E> {
    /// Create a representative usable for operations lacking
    /// side channel protections.  
    pub fn into_vartime(&self) -> KeypairVT<E> {
        let secret = self.secret.into_vartime();
        let public = self.public;
        KeypairVT { secret, public }
    }

    /// Sign a message creating a `Signature` using a user supplied CSPRNG for the key splitting.
    pub fn sign_with_rng<R: Rng>(&mut self, message: &Message, rng: R) -> Signature<E> {
        self.secret.sign(&message, rng)
    }

    /// Sign a message using a Seedabale RNG created from user supplied seed
    pub fn sign_with_random_seed(&mut self, message: &Message, seed: [u8; 32]) -> Signature<E> {
        self.sign_with_rng::<StdRng>(message, SeedableRng::from_seed(seed))
    }

    /// Sign a message using a Seedabale RNG created from a seed derived from the message and key
    pub fn sign(&mut self, message: &Message) -> Signature<E> {
        let mut serialized_part1 = [0u8; 32];
        let mut serialized_part2 = [0u8; 32];
        self.secret.key[0]
            .serialize_compressed(&mut serialized_part1[..])
            .unwrap();
        self.secret.key[1]
            .serialize_compressed(&mut serialized_part2[..])
            .unwrap();

        let seed_digest = Sha256::new()
            .chain_update(serialized_part1)
            .chain_update(serialized_part2)
            .chain_update(message.0);

        let seed: [u8; 32] = seed_digest.finalize().into();

        self.sign_with_rng::<StdRng>(message, SeedableRng::from_seed(seed))
    }

    #[cfg(feature = "std")]
    /// Sign a message creating a `Signature` using the default `ThreadRng`.
    pub fn sign_thread_rng(&mut self, message: &Message) -> Signature<E> {
        self.sign_with_rng(message, thread_rng())
    }

    /// Create a `SignedMessage` using the default `ThreadRng`.
    pub fn signed_message(&mut self, message: &Message) -> SignedMessage<E> {
        let signature = self.sign(&message);
        SignedMessage {
            message: message.clone(),
            publickey: self.public,
            signature,
        }
    }
}

/// Message with attached BLS signature
///
///
#[derive(Debug, Clone)]
pub struct SignedMessage<E: EngineBLS> {
    pub message: Message,
    pub publickey: PublicKey<E>,
    pub signature: Signature<E>,
}
// TODO: Serialization

// borrow_wrapper!(Signature,SignatureGroup,signature);
// borrow_wrapper!(PublicKey,PublicKeyGroup,publickey);

impl<E: EngineBLS> PartialEq<Self> for SignedMessage<E> {
    fn eq(&self, other: &Self) -> bool {
        self.message.eq(&other.message)
            && self.publickey.eq(&other.publickey)
            && self.signature.eq(&other.signature)
    }
}

impl<E: EngineBLS> Eq for SignedMessage<E> {}

impl<'a, E: EngineBLS> Signed for &'a SignedMessage<E> {
    type E = E;

    type M = Message;
    type PKG = PublicKey<E>;

    type PKnM = ::core::iter::Once<(Message, PublicKey<E>)>;

    fn messages_and_publickeys(self) -> Self::PKnM {
        once((self.message.clone(), self.publickey)) // TODO:  Avoid clone
    }

    fn signature(&self) -> Signature<E> {
        self.signature
    }

    fn verify(self) -> bool {
        self.signature.verify(&self.message, &self.publickey)
    }
}

impl<E: EngineBLS> SignedMessage<E> {
    #[cfg(test)]
    pub fn verify_slow(&self) -> bool {
        let g1_one = <E::PublicKeyGroup as CurveGroup>::Affine::generator();
        let message = self.message.hash_to_signature_curve::<E>().into_affine();
        E::pairing(g1_one, self.signature.0.into_affine())
            == E::pairing(self.publickey.0.into_affine(), message)
    }

    /// Hash output from a BLS signature regarded as a VRF.
    ///
    /// If you are not the signer then you must verify the VRF before calling this method.
    ///
    /// If called with distinct contexts then outputs should be independent.
    ///
    /// We incorporate both the input and output to provide the 2Hash-DH
    /// construction from Theorem 2 on page 32 in appendex C of
    /// ["Ouroboros Praos: An adaptively-secure, semi-synchronous proof-of-stake blockchain"](https://eprint.iacr.org/2017/573.pdf)
    /// by Bernardo David, Peter Gazi, Aggelos Kiayias, and Alexander Russell.
    pub fn vrf_hash<H: ExtendableOutput>(&self, h: &mut H) {
        h.update(b"msg");
        h.update(&self.message.0[..]);
        h.update(b"out");
        let affine_signature = self.signature.0.into_affine();
        let mut serialized_signature = vec![0; affine_signature.uncompressed_size()];
        affine_signature
            .serialize_uncompressed(&mut serialized_signature[..])
            .unwrap();

        h.update(&serialized_signature);
    }

    /// Raw bytes output from a BLS signature regarded as a VRF.
    ///
    /// If you are not the signer then you must verify the VRF before calling this method.
    ///
    /// If called with distinct contexts then outputs should be independent.
    pub fn make_bytes<Out: Default + AsMut<[u8]>>(&self, context: &[u8]) -> Out {
        let mut t = Shake128::default();
        t.update(context);
        self.vrf_hash(&mut t);
        let mut seed = Out::default();
        XofReader::read(&mut t.finalize_xof(), seed.as_mut());
        seed
    }

    /* TODO: Switch to this whenever pairing upgrades to rand 0.5 or later
    /// VRF output converted into any `SeedableRng`.
    ///
    /// If you are not the signer then you must verify the VRF before calling this method.
    ///
    /// We expect most users would prefer the less generic `VRFInOut::make_chacharng` method.
    pub fn make_rng<R: SeedableRng>(&self, context: &[u8]) -> R {
        R::from_seed(self.make_bytes::<R::Seed>(context))
    }
    */

    /// VRF output converted into a `ChaChaRng`.
    ///
    /// If you are not the signer then you must verify the VRF before calling this method.
    ///
    /// If called with distinct contexts then outputs should be independent.
    /// Independent output streams are available via `ChaChaRng::set_stream` too.
    ///
    /// We incorporate both the input and output to provide the 2Hash-DH
    /// construction from Theorem 2 on page 32 in appendex C of
    /// ["Ouroboros Praos: An adaptively-secure, semi-synchronous proof-of-stake blockchain"](https://eprint.iacr.org/2017/573.pdf)
    /// by Bernardo David, Peter Gazi, Aggelos Kiayias, and Alexander Russell.
    pub fn make_chacharng(&self, context: &[u8]) -> ChaCha8Rng {
        let bytes = self.make_bytes::<[u8; 32]>(context);
        ChaCha8Rng::from_seed(bytes)
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use ark_bls12_377::Bls12_377;
    use ark_bls12_381::Bls12_381;
    use ark_ec::bls12::Bls12Config;
    use ark_ec::hashing::curve_maps::wb::{WBConfig, WBMap};
    use ark_ec::hashing::map_to_curve_hasher::MapToCurve;
    use ark_ec::pairing::Pairing as PairingEngine;

    use super::*;
    use crate::{CurveExtraConfig, TinyBLS, UsualBLS};

    use core::convert::TryInto;
    use hex_literal::hex;

    fn bls_engine_serialization_test<
        EB: EngineBLS<Engine = E>,
        E: PairingEngine,
        P: Bls12Config + CurveExtraConfig,
    >(
        x: SignedMessage<EB>,
    ) -> SignedMessage<EB>
    where
        <P as Bls12Config>::G2Config: WBConfig,
        WBMap<<P as Bls12Config>::G2Config>: MapToCurve<<E as PairingEngine>::G2>,
    {
        let SignedMessage {
            message,
            publickey,
            signature,
        } = x;

        let publickey = PublicKey::<EB>::from_bytes(&publickey.to_bytes()).unwrap();
        let signature = Signature::<EB>::from_bytes(&signature.to_bytes()).unwrap();

        SignedMessage {
            message,
            publickey,
            signature,
        }
    }

    /// generates a random secret key sign a message and convert the
    /// key to bytes then reconvert it to key and derive its public key
    /// And check if the signature still verifies    
    fn test_serialize_deserialize_production_secret_key<
        E: PairingEngine,
        P: Bls12Config + CurveExtraConfig,
    >()
    where
        <P as Bls12Config>::G2Config: WBConfig,
        WBMap<<P as Bls12Config>::G2Config>: MapToCurve<<E as PairingEngine>::G2>,
    {
        let mut keypair = Keypair::<UsualBLS<E, P>>::generate(thread_rng());
        let serialized_secret_key = keypair.secret.to_bytes();
        println!(
            "secret key serialize size: {}, secret key first scaler serialize size {}",
            keypair.secret.uncompressed_size(),
            keypair.secret.key[0].uncompressed_size()
        );

        let good_message = Message::new(b"ctx", b"test message");

        let sig = keypair.sign(&good_message);

        let deserialized_secret_key =
            SecretKey::<UsualBLS<E, P>>::from_bytes(&serialized_secret_key).unwrap();
        let reconstructed_public_key = deserialized_secret_key.into_public();
        assert!(sig.verify(&good_message, &reconstructed_public_key));
    }

    fn test_deserialize_random_value_as_secret_key_fails<
        E: PairingEngine,
        P: Bls12Config + CurveExtraConfig,
    >(
        random_seed: &[u8],
    ) where
        <P as Bls12Config>::G2Config: WBConfig,
        WBMap<<P as Bls12Config>::G2Config>: MapToCurve<<E as PairingEngine>::G2>,
    {
        match SecretKey::<UsualBLS<E, P>>::from_bytes(
            random_seed
                .try_into()
                .expect("the size of the seed be 32 Bytes."),
        ) {
            Ok(_) => assert!(
                false,
                "random seed should not be canonically deserializable to a secret key."
            ),
            Err(SerializationError::InvalidData) => (),
            _ => assert!(false, "unexpected deserialization error."),
        }
    }

    // fn test_public_key_and_message_serialization<E: PairingEngine, P: Bls12Config>(x: SignedMessage<EB>)-> SignedMessage<E> where <P as Bls12Config>::G2Config: WBConfig, WBMap<<P as Bls12Config>::G2Config>: MapToCurve<<E as PairingEngine>::G2> {
    //     let SignedMessage { message, publickey, signature } = x;
    //     let publickey = PublicKey::<E>::from_bytes(publickey.to_bytes()).unwrap();
    //     let signature = Signature::<E>::from_bytes(signature.to_bytes()).unwrap();
    //     assert!(SignedMessage { message, publickey, signature } == x);
    // }

    fn test_single_bls_message<E: PairingEngine, P: Bls12Config + CurveExtraConfig>()
    where
        <P as Bls12Config>::G2Config: WBConfig,
        WBMap<<P as Bls12Config>::G2Config>: MapToCurve<<E as PairingEngine>::G2>,
    {
        let good = Message::new(b"ctx", b"test message");

        let mut keypair = Keypair::<UsualBLS<E, P>>::generate(thread_rng());
        let good_sig0 = keypair.signed_message(&good);
        let good_sig = bls_engine_serialization_test::<UsualBLS<E, P>, E, P>(good_sig0);
        assert!(good_sig.verify_slow());

        let keypair_vt = keypair.into_vartime();
        assert!(keypair_vt.secret.0 == keypair_vt.into_split(thread_rng()).into_vartime().secret.0);
        assert!(good_sig == keypair.signed_message(&good));
        assert!(good_sig == keypair_vt.signed_message(&good));

        let bad = Message::new(b"ctx", b"wrong message");
        let bad_sig0 = keypair.signed_message(&bad);
        let bad_sig = bls_engine_serialization_test::<UsualBLS<E, P>, E, P>(bad_sig0);
        assert!(bad_sig == keypair.into_vartime().signed_message(&bad));

        assert!(bad_sig.verify());

        let another = Message::new(b"ctx", b"another message");
        let another_sig = keypair.signed_message(&another);
        assert!(another_sig == keypair.into_vartime().signed_message(&another));
        assert!(another_sig.verify());

        assert!(
            keypair.public.verify(&good, &good_sig.signature),
            "Verification of a valid signature failed!"
        );

        assert!(good != bad, "good == bad");
        assert!(
            good_sig.signature != bad_sig.signature,
            "good sig == bad sig"
        );

        assert!(
            !keypair.public.verify(&good, &bad_sig.signature),
            "Verification of a signature on a different message passed!"
        );
        assert!(
            !keypair.public.verify(&bad, &good_sig.signature),
            "Verification of a signature on a different message passed!"
        );
        assert!(
            !keypair.public.verify(
                &Message::new(b"other", b"test message"),
                &good_sig.signature
            ),
            "Verification of a signature on a different message passed!"
        );
    }

    #[test]
    fn zbls_engine_bytes_test() {
        let mut keypair =
            Keypair::<UsualBLS<Bls12_381, ark_bls12_381::Config>>::generate(thread_rng());
        let good_sig0 = keypair.signed_message(&Message::new(b"ctx", b"test message"));

        bls_engine_serialization_test::<
            UsualBLS<Bls12_381, ark_bls12_381::Config>,
            Bls12_381,
            ark_bls12_381::Config,
        >(good_sig0);
    }
    #[test]
    fn bls377_engine_bytes_test() {
        let mut keypair =
            Keypair::<UsualBLS<Bls12_377, ark_bls12_377::Config>>::generate(thread_rng());
        let good_sig0 = keypair.signed_message(&Message::new(b"ctx", b"test message"));

        bls_engine_serialization_test::<
            UsualBLS<Bls12_377, ark_bls12_377::Config>,
            Bls12_377,
            ark_bls12_377::Config,
        >(good_sig0);
    }

    #[test]
    fn tiny_zbls_engine_bytes_test() {
        let mut keypair =
            Keypair::<TinyBLS<Bls12_381, ark_bls12_381::Config>>::generate(thread_rng());
        let good_sig0 = keypair.signed_message(&Message::new(b"ctx", b"test message"));

        bls_engine_serialization_test::<
            TinyBLS<Bls12_381, ark_bls12_381::Config>,
            Bls12_381,
            ark_bls12_381::Config,
        >(good_sig0);
    }

    #[test]
    fn tiny_bls377_engine_bytes_test() {
        let mut keypair =
            Keypair::<TinyBLS<Bls12_377, ark_bls12_377::Config>>::generate(thread_rng());
        let good_sig0 = keypair.signed_message(&Message::new(b"ctx", b"test message"));

        bls_engine_serialization_test::<
            TinyBLS<Bls12_377, ark_bls12_377::Config>,
            Bls12_377,
            ark_bls12_377::Config,
        >(good_sig0);
    }

    #[test]
    fn single_messages_zbls() {
        test_single_bls_message::<Bls12_381, ark_bls12_381::Config>();
    }

    #[test]
    fn single_messages_bls377() {
        test_single_bls_message::<Bls12_377, ark_bls12_377::Config>();
    }

    #[test]
    fn test_secret_key_serialization_for_zbls() {
        test_serialize_deserialize_production_secret_key::<Bls12_381, ark_bls12_381::Config>();
    }

    #[test]
    fn test_secret_key_serialization_for_bls377() {
        test_serialize_deserialize_production_secret_key::<Bls12_377, ark_bls12_377::Config>();
    }

    #[test]
    fn test_deserialize_random_value_as_secret_key_fails_for_bls377() {
        let random_seed = hex!("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60");
        test_deserialize_random_value_as_secret_key_fails::<Bls12_377, ark_bls12_377::Config>(
            random_seed.as_slice(),
        );
    }
}
