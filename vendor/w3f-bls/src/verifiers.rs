//! ## Algorithms for optimized verification of aggregate and batched BLS signatures.
//!
//!
//!

use core::borrow::Borrow;
#[cfg(feature = "std")]
use std::collections::HashMap;

#[cfg(feature = "std")]
use ark_ec::AffineRepr;
#[cfg(feature = "std")]
use ark_ff::field_hashers::{DefaultFieldHasher, HashToField};
#[cfg(feature = "std")]
use ark_serialize::CanonicalSerialize;
#[cfg(feature = "std")]
use digest::DynDigest;

use ark_ec::CurveGroup;

use alloc::vec::Vec;

use super::*;

// We define these convenience type alias here instead of engine.rs
// because seemingly only verifier implementations really employ them.
// And we `pub use engine::*` in lib.rs.

/// Convenience type alias for projective form of `PublicKeyGroup`
pub type PublicKeyProjective<E> = <E as EngineBLS>::PublicKeyGroup;

/// Convenience type alias for affine form of `PublicKeyGroup`
pub type PublicKeyAffine<E> = <<E as EngineBLS>::PublicKeyGroup as CurveGroup>::Affine;

/// Convenience type alias for projective form of `SignatureGroup`
pub type SignatureProjective<E> = <E as EngineBLS>::SignatureGroup;

/// Convenience type alias for affine form of `SignatureGroup`
pub type SignatureAffine<E> = <<E as EngineBLS>::SignatureGroup as CurveGroup>::Affine;

/// Simple unoptimized BLS signature verification.  Useful for testing.
pub fn verify_unoptimized<S: Signed>(s: S) -> bool {
    let signature = S::E::prepare_signature(s.signature().0);
    let prepared = s
        .messages_and_publickeys()
        .map(|(message, public_key)| {
            (
                S::E::prepare_public_key(public_key.borrow().0),
                S::E::prepare_signature(message.borrow().hash_to_signature_curve::<S::E>()),
            )
        })
        .collect::<Vec<(_, _)>>();
    S::E::verify_prepared(signature, prepared.iter())
}

/// Simple universal BLS signature verification
///
/// We support an unstable `Signed::messages_and_publickeys()`
/// securely by calling it only once and batch normalizing all
/// points, as do most other verification routines here.
/// We do no optimizations that reduce the number of pairings
/// by combining repeated messages or signers.
pub fn verify_simple<S: Signed>(s: S) -> bool {
    let signature = s.signature().0;
    // We could write this more idiomatically using iterator adaptors,
    // and avoiding an unecessary allocation for publickeys, but only
    // by calling self.messages_and_publickeys() repeatedly.
    let itr = s.messages_and_publickeys();
    let l = {
        let (lower, upper) = itr.size_hint();
        upper.unwrap_or(lower)
    };
    let mut gpk = Vec::with_capacity(l);
    let mut gms = Vec::with_capacity(l + 1);
    for (message, publickey) in itr {
        gpk.push(publickey.borrow().0.clone());
        gms.push(message.borrow().hash_to_signature_curve::<S::E>());
    }
    let gpk = <<S as Signed>::E as EngineBLS>::PublicKeyGroup::normalize_batch(gpk.as_mut_slice());
    gms.push(signature);
    let mut gms =
        <<S as Signed>::E as EngineBLS>::SignatureGroup::normalize_batch(gms.as_mut_slice());
    let signature = <<S as Signed>::E as EngineBLS>::prepare_signature(gms.pop().unwrap());
    let prepared = gpk
        .iter()
        .zip(gms)
        .map(|(pk, m)| {
            (
                <<S as Signed>::E as EngineBLS>::prepare_public_key(*pk),
                <<S as Signed>::E as EngineBLS>::prepare_signature(m),
            )
        })
        .collect::<Vec<(_, _)>>();
    S::E::verify_prepared(signature, prepared.iter())
}

/// BLS signature verification optimized for all unique messages
///
/// Assuming all messages are distinct, the minimum number of pairings
/// is the number of unique signers, which we achieve here.
/// We do not verify message uniqueness here, but leave this to the
/// aggregate signature type, like `DistinctMessages`.
///
/// We merge any messages with identical signers and batch normalize
/// message points and the signature itself.
/// We optionally batch normalize the public keys in the event that
/// they are provided by algerbaic operaations, but this sounds
/// unlikely given our requirement that messages be distinct.
#[cfg(feature = "std")]
pub fn verify_with_distinct_messages<S: Signed>(signed: S, normalize_public_keys: bool) -> bool {
    let signature = signed.signature().0;
    // We first hash the messages to the signature curve and
    // normalize the public keys to operate on them as bytes.
    // TODO: Assess if we should mutate in place using interior
    // mutability, maybe using `BorrowMut` support in
    // `batch_normalization`.
    let itr = signed.messages_and_publickeys();
    let l = {
        let (lower, upper) = itr.size_hint();
        upper.unwrap_or(lower)
    };
    let mut publickeys = Vec::with_capacity(l);
    let mut messages = Vec::with_capacity(l + 1);
    for (m, pk) in itr {
        publickeys.push(pk.borrow().0.clone());
        messages.push(m.borrow().hash_to_signature_curve::<S::E>());
    }
    let mut affine_publickeys = if normalize_public_keys {
        <<S as Signed>::E as EngineBLS>::PublicKeyGroup::normalize_batch(&publickeys)
    } else {
        publickeys
            .iter()
            .map(|pk| pk.into_affine())
            .collect::<Vec<_>>()
    };

    // We next accumulate message points with the same signer.
    // We could avoid the allocation here if we sorted both
    // arrays in parallel.  This might mean (a) some sort function
    // using `ops::IndexMut` instead of slices, and (b) wrapper types
    // types to make tuples of slices satisfy `ops::IndexMut`.
    // TODO:  Impl PartialEq, Eq, Hash for pairing::EncodedPoint
    // to avoid  struct H(E::PublicKeyGroup::Affine::Uncompressed);
    type AA<E> = (PublicKeyAffine<E>, SignatureProjective<E>);
    let mut pks_n_ms = HashMap::with_capacity(l);
    for (pk, m) in affine_publickeys.drain(..).zip(messages.drain(..)) {
        let mut pk_uncompressed = vec![0; pk.uncompressed_size()];
        pk.serialize_uncompressed(&mut pk_uncompressed[..]).unwrap();
        pks_n_ms
            .entry(pk_uncompressed)
            .and_modify(|(_pk0, m0): &mut AA<S::E>| {
                *m0 += m;
            })
            .or_insert((pk, m));
    }

    let mut publickeys = Vec::with_capacity(l);
    for (_, (pk, m)) in pks_n_ms.drain() {
        messages.push(m);
        publickeys.push(pk.clone());
    }

    // We finally normalize the messages and signature

    messages.push(signature);
    let mut affine_messages =
        <<S as Signed>::E as EngineBLS>::SignatureGroup::normalize_batch(messages.as_mut_slice());
    let signature =
        <<S as Signed>::E as EngineBLS>::prepare_signature(affine_messages.pop().unwrap());
    // TODO: Assess if we could cache normalized message hashes anyplace
    // using interior mutability, but probably this does not work well
    // with ur optimization of collecting messages with thesame signer.

    // And verify the aggregate signature.
    let prepared = publickeys
        .iter()
        .zip(messages)
        .map(|(pk, m)| {
            (
                <<S as Signed>::E as EngineBLS>::prepare_public_key(*pk),
                <<S as Signed>::E as EngineBLS>::prepare_signature(m),
            )
        })
        .collect::<Vec<(_, _)>>();

    S::E::verify_prepared(signature, prepared.iter())

    //let prepared = publickeys.iter().zip(&messages);
    //S::E::verify_prepared( &signature, prepared )
}

/// BLS signature verification optimized for all unique messages
///
/// Assuming all messages are distinct, the minimum number of pairings
/// is the number of unique signers, which we achieve here.
/// We do not verify message uniqueness here, but leave this to the
/// aggregate signature type, like `DistinctMessages`.
///
/// We merge any messages with identical signers and batch normalize
/// message points and the signature itself.
/// We optionally batch normalize the public keys in the event that
/// they are provided by algerbaic operaations, but this sounds
/// unlikely given our requirement that messages be distinct.
#[cfg(feature = "std")]
pub fn verify_using_aggregated_auxiliary_public_keys<
    E: EngineBLS,
    H: DynDigest + Default + Clone,
>(
    signed: &single_pop_aggregator::SignatureAggregatorAssumingPoP<E>,
    normalize_public_keys: bool,
    aggregated_aux_pub_key: <E as EngineBLS>::SignatureGroup,
) -> bool {
    let signature = Signed::signature(&signed).0;

    let mut signature_as_bytes = vec![0; signature.compressed_size()];
    signature
        .serialize_compressed(&mut signature_as_bytes[..])
        .expect("compressed size has been alocated");

    let itr = signed.messages_and_publickeys();
    let l = {
        let (lower, upper) = itr.size_hint();
        upper.unwrap_or(lower)
    };
    let (first_message, first_public_key) = match signed.messages_and_publickeys().next() {
        Some((first_message, first_public_key)) => (first_message, first_public_key),
        None => return false,
    };

    let mut first_public_key_as_bytes = vec![0; first_public_key.compressed_size()];
    first_public_key
        .serialize_compressed(&mut first_public_key_as_bytes[..])
        .expect("compressed size has been alocated");

    let first_message_point = first_message.hash_to_signature_curve::<E>();
    let first_message_point_as_bytes = E::signature_point_to_byte(&first_message_point);

    let mut aggregated_aux_pub_key_as_bytes = vec![0; aggregated_aux_pub_key.compressed_size()];
    aggregated_aux_pub_key
        .serialize_compressed(&mut aggregated_aux_pub_key_as_bytes[..])
        .expect("compressed size has been alocated");

    // We first hash the messages to the signature curve and
    // normalize the public keys to operate on them as bytes.
    // TODO: Assess if we should mutate in place using interior
    // mutability, maybe using `BorrowMut` support in
    // `batch_normalization`.

    // deterministic randomness for adding aggregated auxiliary pub keys
    //TODO you can't just assume that there is one pubickey you need to stop if they were more or aggregate them

    let pseudo_random_scalar_seed = [
        first_message_point_as_bytes,
        first_public_key_as_bytes,
        aggregated_aux_pub_key_as_bytes,
        signature_as_bytes,
    ]
    .concat();

    let hasher = <DefaultFieldHasher<H> as HashToField<E::Scalar>>::new(&[]);
    let pseudo_random_scalar: E::Scalar =
        hasher.hash_to_field(&pseudo_random_scalar_seed[..], 1)[0];

    let signature = signature + aggregated_aux_pub_key * pseudo_random_scalar;

    let mut publickeys = Vec::with_capacity(l);
    let mut messages = Vec::with_capacity(l + 1);

    //Simplify from here on.
    for (m, pk) in itr {
        publickeys.push(pk.0.clone());
        messages.push(
            m.hash_to_signature_curve::<E>()
                + E::SignatureGroupAffine::generator() * pseudo_random_scalar,
        );
    }

    let mut affine_publickeys = if normalize_public_keys {
        <E as EngineBLS>::PublicKeyGroup::normalize_batch(&publickeys)
    } else {
        publickeys
            .iter()
            .map(|pk| pk.into_affine())
            .collect::<Vec<_>>()
    };

    // We next accumulate message points with the same signer.
    // We could avoid the allocation here if we sorted both
    // arrays in parallel.  This might mean (a) some sort function
    // using `ops::IndexMut` instead of slices, and (b) wrapper types
    // types to make tuples of slices satisfy `ops::IndexMut`.
    // TODO:  Impl PartialEq, Eq, Hash for pairing::EncodedPoint
    // to avoid  struct H(E::PublicKeyGroup::Affine::Uncompressed);
    type AA<E> = (PublicKeyAffine<E>, SignatureProjective<E>);
    let mut pks_n_ms = HashMap::with_capacity(l);
    for (pk, m) in affine_publickeys.drain(..).zip(messages.drain(..)) {
        let mut pk_uncompressed = vec![0; pk.uncompressed_size()];
        pk.serialize_uncompressed(&mut pk_uncompressed[..]).unwrap();
        pks_n_ms
            .entry(pk_uncompressed)
            .and_modify(|(_pk0, m0): &mut AA<E>| {
                *m0 += m;
            })
            .or_insert((pk, m));
    }

    let mut publickeys = Vec::with_capacity(l);
    for (_, (pk, m)) in pks_n_ms.drain() {
        messages.push(m);
        publickeys.push(pk.clone());
    }

    // We finally normalize the messages and signature

    messages.push(signature);
    let mut affine_messages =
        <E as EngineBLS>::SignatureGroup::normalize_batch(messages.as_mut_slice());
    let signature = <E as EngineBLS>::prepare_signature(affine_messages.pop().unwrap());
    // TODO: Assess if we could cache normalized message hashes anyplace
    // using interior mutability, but probably this does not work well
    // with ur optimization of collecting messages with thesame signer.

    // And verify the aggregate signature.
    let prepared = publickeys
        .iter()
        .zip(messages)
        .map(|(pk, m)| {
            (
                <E as EngineBLS>::prepare_public_key(*pk),
                <E as EngineBLS>::prepare_signature(m),
            )
        })
        .collect::<Vec<(_, _)>>();

    E::verify_prepared(signature, prepared.iter())

    //let prepared = publickeys.iter().zip(&messages);
    //S::E::verify_prepared( &signature, prepared )
}

/*
/// Excessively optimized BLS signature verification
///
/// We minimize the number of pairing operations by doing two
/// basis change operation using Gaussian elimination, first in the
/// message space and then in the signer space.  As a result, we
/// do only `1 + min(msg_d,pk_d)` pairings where `msg_d` and `pk_d`
/// are the numbers of distinct messages and signers, respectively.
///
/// We expect this to improve performance dramatically when both
/// signers and messages are repeated enough, simpler strategies
/// work as well or better when say messages are distinct.
///
/// Explination:
///
/// We consider the bipartite graph with vertex sets given by points
/// on the two curves and edges given by desired pairings between them.
/// We let $M$ denote the bipartite adjacency matrix for this graph,
/// so that multiplying $M$ on the the right and left by the vectors
/// of messages and signers respectively reproduces our original sum
/// of pairings.
///
/// We first use elementary "row" operations to make $M$ upper
/// triangular, as in Gaussian elimination, but at the cost of also
/// performing one-sided "change of basis" operations that collect
/// our original "basis vectors" into sums of curve points.
/// We next use elementary "column" operations to make $M$ diagonal,
/// again adjusting the basis with curve point operations.
///
/// In this, we regard $M$ as a matrix over the scalar field $F_p$
/// so we may do row or column swaps and row or column addition
/// operations with small scalars, but not lone row or column scalar
/// multiplication because these always involve divisions, which
/// produces large curve points that slow us down thereafter.
/// We do not require such divisions because we do not solve any
/// system of equations and do not need ones on the diagonal.
///
/// TODO:
/// We leave implementing this optimization to near future work
/// because it benifits from public keys being affine or having
/// another hashable representation.
///
///
/// As a curiosity, we note one interesting but suboptimal algorithm
/// that avoids small scalar multiplications when doing this:
///
/// If we ignore subtraction, then the minimal number of pairing
/// operations required to verify aggregated BLS signatures is the
/// minimal bipartite edge cover, aka bipartite dimension, of the
/// bipartite graph with vertices given by points on the two curves
/// and edges given by desired pairings.
/// In general, this problem is NP-hard even to approximate.
/// See:  https://en.wikipedia.org/wiki/Bipartite_dimension
///
/// There are polynomial time algorithms for bipartite edge cover in
/// special cases, with domino-free graphs being among the widest
/// known classes.  See:
/// Amilhastre, Jérôme; Janssen, Philippe; Vilarem, Marie-Catherine,
/// "Computing a minimum biclique cover is polynomial for bipartite domino-free graphs" (1997)
/// https://core.ac.uk/download/pdf/82546650.pdf
///
/// If we now exploit subtraction, then these dominos can be
/// completed into $K_{3,3}$s, like
///  $(a,x)+(a,y)+(b,x)+(b,y)+(b,z)+(c,y)+(c,z) = (a+b+c,x+y+z) - (a,z) - (c,z)$
/// which looks optimal for itself, and likely permits the further
/// aggregation, and maybe the subtracted terms can be aggregated later.
///
/// We could not however find the optimal numbers of pairings by
/// completing dominos like this because (a+b+c,x+y+z) - (b,y),
/// which looks optimal for itself, but only has one subtraction.
fn verify_with_gaussian_elimination<S: Signed>(s: S) -> bool {
    unimplemented!()
}

*/
