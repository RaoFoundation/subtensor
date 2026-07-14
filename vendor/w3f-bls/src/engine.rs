//! ## Adaptation of `ark_ec::PairingEngine` to BLS-like signatures.
//!
//! We provide an `EngineBLS` trait that adapts `pairing::Engine`
//! to BLS-like signatures by permitting the group roles to be
//! transposed, which involves removing the field of definition,
//! while retaining the correct associations.  
//!
//! We support same-message aggregation strategies using wrappers
//! that satisfy `EngineBLS` as well, primarily because these
//! strategies must ocntroll access to the public key type.
//!
//! In future, we should support [Pixel](https://github.com/w3f/bls/issues/4)
//! by adding wrapper that replace `SignatureGroup` with a product
//! of both groups.  I think this requires abstracting `CruveAffine`
//! and `CruveProjective` without their base fields and wNAF windows,
//! but still with their affine, projective, and compressed forms,
//! and batch normalization.

use core::borrow::Borrow;
use core::ops::MulAssign;

use alloc::{vec, vec::Vec};

use ark_ec::hashing::curve_maps::wb::{WBConfig, WBMap};
use ark_ec::hashing::{
    map_to_curve_hasher::{MapToCurve, MapToCurveBasedHasher},
    HashToCurve,
};
use ark_ec::{
    pairing::{MillerLoopOutput, Pairing, PairingOutput},
    AffineRepr, CurveGroup,
};
use ark_ff::field_hashers::{DefaultFieldHasher, HashToField};
use ark_ff::{Field, PrimeField, UniformRand, Zero};
use ark_serialize::CanonicalSerialize;
use rand::Rng;
use rand_core::RngCore;

use core::fmt::Debug;

use sha2::Sha256; //IETF standard asks for SHA256

use ark_ec::bls12::Bls12Config;
use core::marker::PhantomData;

// Expand SHA256 from 256 bits to 1024 bits.
// let output_bits = 1024;
// let output_bytes = 1024 / 8;
// let mut hasher = FullDomainHash::<Sha256>::new(output_bytes).unwrap();
// hasher.update(b"ATTACK AT DAWN");
// let result = hasher.finalize_boxed().into_vec();

/// A weakening of `pairing::Engine` to permit transposing the groups.
///
/// You cannot transpose the two groups in a `pairing::Engine` without
/// first providing panicing implementations of `pairing::PrimeField`
/// for `Engine::Fqe`, which is not a prime field, and second,
/// providing wrapper types for the projective and affine group
/// representations, which makes interacting with the original
/// `pairing::Engine` annoying.  This trait merely replicates
/// transposable functionality from `pairing::Engine` by removing
/// the fields of definition, but leaves the actual BLS signature
/// scheme to wrapper types.
///
/// We also extract two functions users may with to override:
/// random scalar generation and hashing to the singature curve.
pub trait EngineBLS {
    type Engine: Pairing; //<Fr = Self::Scalar>;
    type Scalar: PrimeField; // = <Self::Engine as ScalarEngine>::Fr;
    /// Group where BLS public keys live
    ///
    /// You should take this to be the `Engine::G1` curve usually
    /// becuase all verifiers perform additions on this curve, or
    /// even scalar multiplicaitons with delinearization.
    type PublicKeyGroupBaseField: Field;
    type PublicKeyGroupAffine: AffineRepr<ScalarField = Self::Scalar, Group = Self::PublicKeyGroup>
        + From<Self::PublicKeyGroup>
        + Into<Self::PublicKeyGroup>
        + Into<Self::PublicKeyPrepared>;
    //+ Into<<Self::PublicKeyGroup as CurveGroup>::Affine>;

    type PublicKeyGroup: CurveGroup<
            Affine = Self::PublicKeyGroupAffine,
            ScalarField = Self::Scalar,
            BaseField = Self::PublicKeyGroupBaseField,
        > + From<Self::PublicKeyGroupAffine>
        + Into<Self::PublicKeyGroupAffine>
        + MulAssign<Self::Scalar>;

    type PublicKeyPrepared: Default + Clone + Send + Sync + Debug + From<Self::PublicKeyGroupAffine>;

    const PUBLICKEY_SERIALIZED_SIZE: usize;
    const SECRET_KEY_SIZE: usize;

    // See https://www.ietf.org/archive/id/draft-irtf-cfrg-bls-signature-05.html#name-ciphersuites
    const CURVE_NAME: &'static [u8];
    const SIG_GROUP_NAME: &'static [u8];
    const CIPHER_SUIT_DOMAIN_SEPARATION: &'static [u8];

    /// Group where BLS signatures live
    ///
    /// You should take this to be the `Engine::G2` curve usually
    /// becuase only aggregators perform additions on this curve, or
    /// scalar multiplicaitons with delinearization.
    type SignatureGroupBaseField: Field;

    type SignatureGroupAffine: AffineRepr<ScalarField = Self::Scalar, Group = Self::SignatureGroup>
        + From<Self::SignatureGroup>
        + Into<Self::SignatureGroup>
        + Into<Self::SignaturePrepared>;

    type SignatureGroup: CurveGroup<
            Affine = Self::SignatureGroupAffine,
            ScalarField = Self::Scalar,
            BaseField = Self::SignatureGroupBaseField,
        > + Into<Self::SignatureGroupAffine>
        + From<Self::SignatureGroupAffine>
        + MulAssign<Self::Scalar>;

    type SignaturePrepared: Default + Clone + Send + Sync + Debug + From<Self::SignatureGroupAffine>;

    const SIGNATURE_SERIALIZED_SIZE: usize;

    type HashToSignatureField: HashToField<Self::SignatureGroupBaseField>;
    type MapToSignatureCurve: MapToCurve<Self::SignatureGroup>;

    /// Generate a random scalar for use as a secret key.
    fn generate<R: Rng + RngCore>(rng: &mut R) -> Self::Scalar {
        Self::Scalar::rand(rng)
    }

    /// getter function for the hash to curve map
    fn hash_to_curve_map() -> MapToCurveBasedHasher<
        Self::SignatureGroup,
        Self::HashToSignatureField,
        Self::MapToSignatureCurve,
    >;

    /// Hash one message to the signature curve.
    fn hash_to_signature_curve<M: Borrow<[u8]>>(message: M) -> Self::SignatureGroup {
        Self::hash_to_curve_map()
            .hash(message.borrow())
            .unwrap()
            .into_group()
    }

    /// Run the Miller loop from `Engine` but orients its arguments
    /// to be a `SignatureGroup` and `PublicKeyGroup`.
    fn miller_loop<'a, I>(i: I) -> MillerLoopOutput<Self::Engine>
    where
        Self::PublicKeyPrepared: 'a,
        Self::SignaturePrepared: 'a,
        I: IntoIterator<
            Item = &'a (
                <Self as EngineBLS>::PublicKeyPrepared,
                Self::SignaturePrepared,
            ),
        >;

    /// Perform final exponentiation on the result of a Miller loop.
    fn final_exponentiation(
        e: MillerLoopOutput<Self::Engine>,
    ) -> Option<PairingOutput<Self::Engine>> {
        Self::Engine::final_exponentiation(e)
    }

    /// Performs a pairing operation `e(p, q)` by calling `Engine::pairing`
    /// but orients its arguments to be a `PublicKeyGroup` and `SignatureGroup`.
    fn pairing<G1, G2>(p: G1, q: G2) -> <Self::Engine as Pairing>::TargetField
    where
        G1: Into<<Self::PublicKeyGroup as CurveGroup>::Affine>,
        G2: Into<<Self::SignatureGroup as CurveGroup>::Affine>;
    /*
    {
        Self::final_exponentiation(&Self::miller_loop(
            [(&(p.into().prepare()), &(q.into().prepare()))].into_iter(),
        )).unwrap()
    }
    */

    /// Implement verification equation for aggregate BLS signatures
    /// provided as prepared points
    ///
    /// This low-level routine does no verification of critical security
    /// properties like message distinctness.  It exists purely to
    /// simplify replacing mid-level routines with optimized variants,
    /// like versions that cache public key preperation or use fewer pairings.
    fn verify_prepared<'a, I>(signature: Self::SignaturePrepared, inputs: I) -> bool
    where
        Self::PublicKeyPrepared: 'a,
        Self::SignaturePrepared: 'a,
        I: IntoIterator<Item = &'a (Self::PublicKeyPrepared, Self::SignaturePrepared)>,
    {
        let lhs: [_; 1] = [(
            Self::minus_generator_of_public_key_group_prepared(),
            signature,
        )];
        Self::final_exponentiation(Self::miller_loop(inputs.into_iter().map(|t| t).chain(&lhs)))
            .unwrap()
            == (PairingOutput::<Self::Engine>::zero()) //zero is the target_field::one !!
    }

    /// Prepared negative of the generator of the public key curve.
    fn minus_generator_of_public_key_group_prepared() -> Self::PublicKeyPrepared;

    /// return the generator of signature group
    fn generator_of_signature_group() -> Self::SignatureGroup {
        <Self::SignatureGroup as CurveGroup>::Affine::generator().into()
    }

    /// Process the public key to be use in pairing. This has to be
    /// implemented by the type of BLS system implementing the engine
    /// by calling either prepare_g1 or prepare_g2 based on which group
    /// is used by the signature system to host the public key
    fn prepare_public_key(g: impl Into<Self::PublicKeyGroupAffine>) -> Self::PublicKeyPrepared {
        let g_affine: Self::PublicKeyGroupAffine = g.into();
        Self::PublicKeyPrepared::from(g_affine)
    }

    /// Process the signature to be use in pairing. This has to be
    /// implemented by the type of BLS system implementing the engine
    /// by calling either prepare_g1 or prepare_g2 based on which group
    /// is used by the signature system to host the public key
    fn prepare_signature(g: impl Into<Self::SignatureGroupAffine>) -> Self::SignaturePrepared {
        let g_affine: Self::SignatureGroupAffine = g.into();
        Self::SignaturePrepared::from(g_affine)
    }

    /// Serialization helper for various sigma protocols
    fn signature_point_to_byte(point: &Self::SignatureGroup) -> Vec<u8> {
        let mut point_as_bytes = vec![0; Self::SIGNATURE_SERIALIZED_SIZE];
        let point_affine = point.into_affine();
        point_affine
            .serialize_compressed(&mut point_as_bytes[..])
            .unwrap();
        point_as_bytes
    }

    fn public_key_point_to_byte(point: &Self::PublicKeyGroup) -> Vec<u8> {
        let mut point_as_bytes = vec![0; Self::PUBLICKEY_SERIALIZED_SIZE];
        let point_affine = point.into_affine();
        point_affine
            .serialize_compressed(&mut point_as_bytes[..])
            .unwrap();
        point_as_bytes
    }
}

/// Usual aggregate BLS signature scheme on ZCash's BLS12-381 curve.
pub type ZBLS = UsualBLS<ark_bls12_381::Bls12_381, ark_bls12_381::Config>;
pub type BLS377 = UsualBLS<ark_bls12_377::Bls12_377, ark_bls12_377::Config>;

/// Usual aggregate BLS signature scheme on ZCash's BLS12-381 curve.
// pub const Z_BLS : ZBLS = UsualBLS(::zexe_algebra::bls12_381::Bls12_381{});

/// Usual BLS variant with tiny 48 byte public keys and 96 byte signatures.
///
/// We favor this variant because verifiers always perform
/// `O(signers)` additions on the `PublicKeyGroup`, or worse 128 bit
/// scalar multiplications with delinearization.
/// We also orient this variant to match zcash's traits.
#[derive(Default)]
pub struct UsualBLS<E: Pairing, P: Bls12Config + CurveExtraConfig>(pub E, PhantomData<fn() -> P>)
where
    <P as Bls12Config>::G2Config: WBConfig,
    WBMap<<P as Bls12Config>::G2Config>: MapToCurve<<E as Pairing>::G2>;

impl<E: Pairing, P: Bls12Config + CurveExtraConfig> EngineBLS for UsualBLS<E, P>
where
    <P as Bls12Config>::G2Config: WBConfig,
    WBMap<<P as Bls12Config>::G2Config>: MapToCurve<<E as Pairing>::G2>,
{
    type Engine = E;
    type Scalar = <Self::Engine as Pairing>::ScalarField;

    type PublicKeyGroup = E::G1;
    type PublicKeyGroupAffine = E::G1Affine;
    type PublicKeyPrepared = E::G1Prepared;
    type PublicKeyGroupBaseField = <<E as Pairing>::G1 as CurveGroup>::BaseField;

    const PUBLICKEY_SERIALIZED_SIZE: usize = 48;
    const SECRET_KEY_SIZE: usize = 32;

    const CURVE_NAME: &'static [u8] = P::CURVE_NAME;
    const SIG_GROUP_NAME: &'static [u8] = b"G2";
    const CIPHER_SUIT_DOMAIN_SEPARATION: &'static [u8] = b"_XMD:SHA-256_SSWU_RO_";

    type SignatureGroup = E::G2;
    type SignatureGroupAffine = E::G2Affine;
    type SignaturePrepared = E::G2Prepared;
    type SignatureGroupBaseField = <<E as Pairing>::G2 as CurveGroup>::BaseField;

    const SIGNATURE_SERIALIZED_SIZE: usize = 96;

    type HashToSignatureField = DefaultFieldHasher<Sha256, 128>;
    type MapToSignatureCurve = WBMap<P::G2Config>;

    fn miller_loop<'a, I>(i: I) -> MillerLoopOutput<E>
    where
        // Self::PublicKeyPrepared: 'a,
        // Self::SignaturePrepared: 'a,
        I: IntoIterator<Item = &'a (Self::PublicKeyPrepared, Self::SignaturePrepared)>,
    {
        let (i_a, i_b): (Vec<Self::PublicKeyPrepared>, Vec<Self::SignaturePrepared>) =
            i.into_iter().cloned().unzip();

        E::multi_miller_loop(i_a, i_b)
    }

    fn pairing<G1, G2>(p: G1, q: G2) -> E::TargetField
    where
        G1: Into<E::G1Affine>,
        G2: Into<E::G2Affine>,
    {
        E::pairing(p.into(), q.into()).0
    }

    /// Prepared negative of the generator of the public key curve.
    fn minus_generator_of_public_key_group_prepared() -> Self::PublicKeyPrepared {
        let g1_minus_generator = <Self::PublicKeyGroup as CurveGroup>::Affine::generator();
        <Self::PublicKeyGroup as Into<Self::PublicKeyPrepared>>::into(
            -g1_minus_generator.into_group(),
        )
    }

    fn hash_to_curve_map() -> MapToCurveBasedHasher<
        Self::SignatureGroup,
        Self::HashToSignatureField,
        Self::MapToSignatureCurve,
    > {
        MapToCurveBasedHasher::<
            Self::SignatureGroup,
            DefaultFieldHasher<Sha256, 128>,
            WBMap<P::G2Config>,
        >::new(&[1])
        .unwrap()
    }
}

/// Infrequently used BLS variant with tiny 48 byte signatures and 96 byte public keys,
///
/// We recommend gainst this variant by default because verifiers
/// always perform `O(signers)` additions on the `PublicKeyGroup`,
/// or worse 128 bit scalar multiplications with delinearization.
/// Yet, there are specific use cases where this variant performs
/// better.  We swapy two group roles relative to zcash here.
#[derive(Default)]
pub struct TinyBLS<E: Pairing, P: Bls12Config + CurveExtraConfig>(pub E, PhantomData<fn() -> P>)
where
    <P as Bls12Config>::G1Config: WBConfig,
    WBMap<<P as Bls12Config>::G1Config>: MapToCurve<<E as Pairing>::G1>;

/// Trait to add extra config for a curve which is not in ArkWorks library
pub trait CurveExtraConfig {
    const CURVE_NAME: &'static [u8];
}

impl<E: Pairing, P: Bls12Config + CurveExtraConfig> EngineBLS for TinyBLS<E, P>
where
    <P as Bls12Config>::G1Config: WBConfig,
    WBMap<<P as Bls12Config>::G1Config>: MapToCurve<<E as Pairing>::G1>,
{
    type Engine = E;
    type Scalar = <Self::Engine as Pairing>::ScalarField;

    type SignatureGroup = E::G1;
    type SignatureGroupAffine = E::G1Affine;
    type SignaturePrepared = E::G1Prepared;
    type SignatureGroupBaseField = <<E as Pairing>::G1 as CurveGroup>::BaseField;

    const SIGNATURE_SERIALIZED_SIZE: usize = 48;

    type PublicKeyGroup = E::G2;
    type PublicKeyGroupAffine = E::G2Affine;
    type PublicKeyPrepared = E::G2Prepared;
    type PublicKeyGroupBaseField = <<E as Pairing>::G2 as CurveGroup>::BaseField;

    const PUBLICKEY_SERIALIZED_SIZE: usize = 96;
    const SECRET_KEY_SIZE: usize = 32;

    const CURVE_NAME: &'static [u8] = P::CURVE_NAME;
    const SIG_GROUP_NAME: &'static [u8] = b"G1";
    const CIPHER_SUIT_DOMAIN_SEPARATION: &'static [u8] = b"_XMD:SHA-256_SSWU_RO_";

    type HashToSignatureField = DefaultFieldHasher<Sha256, 128>;
    type MapToSignatureCurve = WBMap<P::G1Config>;

    fn miller_loop<'a, I>(i: I) -> MillerLoopOutput<E>
    where
        I: IntoIterator<Item = &'a (Self::PublicKeyPrepared, Self::SignaturePrepared)>,
    {
        // We require an ugly unecessary allocation here because
        // zcash's pairing library cnsumes an iterator of references
        // to tuples of references, which always requires
        let (i_a, i_b): (Vec<Self::PublicKeyPrepared>, Vec<Self::SignaturePrepared>) =
            i.into_iter().cloned().unzip();

        E::multi_miller_loop(i_b, i_a) //in Tiny BLS signature is in G1
    }

    fn pairing<G2, G1>(p: G2, q: G1) -> E::TargetField
    where
        G1: Into<E::G1Affine>,
        G2: Into<E::G2Affine>,
    {
        E::pairing(q.into(), p.into()).0
    }

    /// Prepared negative of the generator of the public key curve.
    fn minus_generator_of_public_key_group_prepared() -> Self::PublicKeyPrepared {
        let g2_minus_generator = <Self::PublicKeyGroup as CurveGroup>::Affine::generator();
        <Self::PublicKeyGroup as Into<Self::PublicKeyPrepared>>::into(
            -g2_minus_generator.into_group(),
        )
    }

    fn hash_to_curve_map() -> MapToCurveBasedHasher<
        Self::SignatureGroup,
        Self::HashToSignatureField,
        Self::MapToSignatureCurve,
    > {
        MapToCurveBasedHasher::<
            Self::SignatureGroup,
            DefaultFieldHasher<Sha256, 128>,
            WBMap<P::G1Config>,
        >::new(&[1])
        .unwrap()
    }
}

/// Aggregate BLS signature scheme with Signature in G1 for BLS12-377 curve.
impl CurveExtraConfig for ark_bls12_377::Config {
    const CURVE_NAME: &'static [u8] = b"BLS12377";
}
pub type TinyBLS377 = TinyBLS<ark_bls12_377::Bls12_377, ark_bls12_377::Config>;
/// Aggregate BLS signature scheme with Signature in G1 for BLS12-381 curve.
impl CurveExtraConfig for ark_bls12_381::Config {
    const CURVE_NAME: &'static [u8] = b"BLS12381";
}
pub type TinyBLS381 = TinyBLS<ark_bls12_381::Bls12_381, ark_bls12_381::Config>;
