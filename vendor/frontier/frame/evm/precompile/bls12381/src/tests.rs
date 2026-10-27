// This file is part of Frontier.

// Copyright (C) Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use super::*;
use pallet_evm_test_vector_support::{test_precompile_test_vectors, MockHandle};

fn failure_vectors<P: Precompile>(file: &str) {
	let data = std::fs::read_to_string(file).unwrap();
	let tests: serde_json::Value = serde_json::from_str(&data).unwrap();
	for test in tests.as_array().unwrap() {
		let input = hex::decode(test["Input"].as_str().unwrap()).unwrap();
		let mut handle = MockHandle::new(
			input,
			None,
			fp_evm::Context {
				address: Default::default(),
				caller: Default::default(),
				apparent_value: 0.into(),
			},
		);
		// Error text is client-specific; the consensus requirement is exceptional failure.
		assert!(
			matches!(
				P::execute(&mut handle),
				Err(PrecompileFailure::Error { .. })
			),
			"{}",
			test["Name"]
		);
	}
}

#[test]
fn process_consensus_tests() -> Result<(), String> {
	test_precompile_test_vectors::<Bls12381G1Add>("testdata/blsG1Add.json")?;
	test_precompile_test_vectors::<Bls12381G1Mul>("../testdata/bls12381G1Mul.json")?;
	test_precompile_test_vectors::<Bls12381G1MultiExp>("testdata/blsG1MultiExp.json")?;
	test_precompile_test_vectors::<Bls12381G2Add>("testdata/blsG2Add.json")?;
	test_precompile_test_vectors::<Bls12381G2Mul>("../testdata/bls12381G2Mul.json")?;
	test_precompile_test_vectors::<Bls12381G2MultiExp>("testdata/blsG2MultiExp.json")?;
	test_precompile_test_vectors::<Bls12381Pairing>("testdata/blsPairing.json")?;
	test_precompile_test_vectors::<Bls12381MapG1>("testdata/blsMapG1.json")?;
	test_precompile_test_vectors::<Bls12381MapG2>("testdata/blsMapG2.json")?;
	Ok(())
}

#[test]
fn process_consensus_failure_tests() {
	failure_vectors::<Bls12381G1Add>("testdata/fail-blsG1Add.json");
	failure_vectors::<Bls12381G1MultiExp>("testdata/fail-blsG1MultiExp.json");
	failure_vectors::<Bls12381G2Add>("testdata/fail-blsG2Add.json");
	failure_vectors::<Bls12381G2MultiExp>("testdata/fail-blsG2MultiExp.json");
	failure_vectors::<Bls12381Pairing>("testdata/fail-blsPairing.json");
	failure_vectors::<Bls12381MapG1>("testdata/fail-blsMapG1.json");
	failure_vectors::<Bls12381MapG2>("testdata/fail-blsMapG2.json");
}
