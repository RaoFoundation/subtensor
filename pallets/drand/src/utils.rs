/*
 * Copyright 2024 by Ideal Labs, LLC
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

//! Host-function / arkworks argument builders used by benchmarks and local crypto tests.

#![allow(dead_code)]

use crate::verifier::ArkScale;
use ark_ec::AffineRepr;
use ark_scale::hazmat::ArkScaleProjective;
use ark_serialize::{CanonicalSerialize, Compress};
use ark_std::{UniformRand, test_rng, vec, vec::Vec};
/// Scalar field of an affine curve point group.
pub type ScalarFieldFor<AffineT> = <AffineT as AffineRepr>::ScalarField;

/// Random scalar as `words_count` little-endian `u64` limbs with MSB of the first limb set.
///
/// Arkworks treats the limb vector as **big endian** for scalar encoding.
fn random_scalar_words(words_count: u32) -> Vec<u64> {
    let mut scalar: Vec<_> = (0..words_count as usize)
        .map(|_| u64::rand(&mut test_rng()))
        .collect();
    // Arkworks assumes scalar to be in **big endian**
    scalar[0] |= 1 << 63;
    scalar
}

/// Uniform random element of `Group` from the test RNG.
fn random_group_element<Group: UniformRand>() -> Group {
    Group::rand(&mut test_rng())
}

/// Pair `(base, scalar_limbs)` for scalar-mul host-function inputs.
pub fn make_scalar_args<Group: UniformRand>(
    words_count: u32,
) -> (ArkScale<Group>, ArkScale<Vec<u64>>) {
    (
        random_group_element::<Group>().into(),
        random_scalar_words(words_count).into(),
    )
}

/// Projective variant of [`make_scalar_args`].
pub fn make_scalar_args_projective<Group: UniformRand>(
    words_count: u32,
) -> (ArkScaleProjective<Group>, ArkScale<Vec<u64>>) {
    (
        random_group_element::<Group>().into(),
        random_scalar_words(words_count).into(),
    )
}

/// Pair of random points for pairing host-function inputs.
pub fn make_pairing_args<GroupA: UniformRand, GroupB: UniformRand>()
-> (ArkScale<GroupA>, ArkScale<GroupB>) {
    (
        random_group_element::<GroupA>().into(),
        random_group_element::<GroupB>().into(),
    )
}

/// Random MSM bases and scalars of the given `size`.
pub fn make_msm_args<Group: ark_ec::VariableBaseMSM>(
    size: u32,
) -> (ArkScale<Vec<Group>>, ArkScale<Vec<Group::ScalarField>>) {
    let rng = &mut test_rng();
    let scalars = (0..size)
        .map(|_| Group::ScalarField::rand(rng))
        .collect::<Vec<_>>();
    let bases = (0..size).map(|_| Group::rand(rng)).collect::<Vec<_>>();
    let bases: ArkScale<Vec<Group>> = bases.into();
    let scalars: ArkScale<Vec<Group::ScalarField>> = scalars.into();
    (bases, scalars)
}

/// Uncompressed canonical serialization of an arkworks argument.
pub fn serialize_argument(argument: impl CanonicalSerialize) -> Vec<u8> {
    let mut buf = vec![0; argument.serialized_size(Compress::No)];
    argument
        .serialize_uncompressed(buf.as_mut_slice())
        .unwrap_or_default();
    buf
}
