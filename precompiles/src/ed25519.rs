extern crate alloc;

use alloc::vec::Vec;
use core::marker::PhantomData;

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use fp_evm::{ExitError, ExitSucceed, LinearCostPrecompile, PrecompileFailure};

use crate::{PrecompileExt, parse_slice};

pub struct Ed25519Verify<A>(PhantomData<A>);

impl<A> PrecompileExt<A> for Ed25519Verify<A>
where
    A: From<[u8; 32]>,
{
    const INDEX: u64 = 1026;
}

impl<A> LinearCostPrecompile for Ed25519Verify<A>
where
    A: From<[u8; 32]>,
{
    // https://eips.ethereum.org/EIPS/eip-665#gas-costs
    // According to the EIP, the base cost should be 2000 gas, less than ECDSA/secp256k1 which is 3000.
    // Reality: Ed25519 verification is ~2.3x more computationally expensive than ECDSA/secp256k1
    // So we set the base cost to 6000 gas, which is 3x of the EIP's base cost.
    const BASE: u64 = 6000;
    const WORD: u64 = 0;

    fn execute(input: &[u8], _: u64) -> Result<(ExitSucceed, Vec<u8>), PrecompileFailure> {
        if input.len() < 132 {
            return Err(PrecompileFailure::Error {
                exit_status: ExitError::Other("input must contain 128 bytes".into()),
            });
        };

        let mut buf = [0u8; 32];

        let msg = parse_slice(input, 4, 36)?;
        let pk = VerifyingKey::try_from(parse_slice(input, 36, 68)?).map_err(|_| {
            PrecompileFailure::Error {
                exit_status: ExitError::Other("Public key recover failed".into()),
            }
        })?;
        let sig = Signature::try_from(parse_slice(input, 68, 132)?).map_err(|_| {
            PrecompileFailure::Error {
                exit_status: ExitError::Other("Signature recover failed".into()),
            }
        })?;

        if pk.verify(msg, &sig).is_ok() {
            buf[31] = 1u8;
        };

        Ok((ExitSucceed::Returned, buf.to_vec()))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::mock::{
        AccountId, abi_word, addr_from_index, new_test_ext, precompiles, selector_u32,
    };
    use precompile_utils::solidity::encode_with_selector;
    use precompile_utils::testing::PrecompileTesterExt;
    use sp_core::{H256, Pair, U256, ed25519};

    #[test]
    fn ed25519_precompile_verifies_valid_and_invalid_signatures() {
        new_test_ext().execute_with(|| {
            let caller = addr_from_index(1);
            let precompile_addr = addr_from_index(Ed25519Verify::<AccountId>::INDEX);

            let pair = ed25519::Pair::from_seed(&[1u8; 32]);
            let message = [7u8; 32];
            let signature = pair.sign(&message);
            let public_key = pair.public();
            let broken_message = [8u8; 32];
            let mut broken_signature = signature.0;
            broken_signature[0] ^= 1;
            let broken_signature = ed25519::Signature::from_raw(broken_signature);

            precompiles::<Ed25519Verify<AccountId>>()
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("verify(bytes32,bytes32,bytes32,bytes32)"),
                        (
                            H256::from(message),
                            H256::from(public_key.0),
                            H256::from_slice(&signature.0[..32]),
                            H256::from_slice(&signature.0[32..]),
                        ),
                    ),
                )
                .with_static_call(true)
                .execute_returns_raw(abi_word(U256::one()));
            precompiles::<Ed25519Verify<AccountId>>()
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("verify(bytes32,bytes32,bytes32,bytes32)"),
                        (
                            H256::from(broken_message),
                            H256::from(public_key.0),
                            H256::from_slice(&signature.0[..32]),
                            H256::from_slice(&signature.0[32..]),
                        ),
                    ),
                )
                .with_static_call(true)
                .execute_returns_raw(abi_word(U256::zero()));
            precompiles::<Ed25519Verify<AccountId>>()
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("verify(bytes32,bytes32,bytes32,bytes32)"),
                        (
                            H256::from(message),
                            H256::from(public_key.0),
                            H256::from_slice(&broken_signature.0[..32]),
                            H256::from_slice(&broken_signature.0[32..]),
                        ),
                    ),
                )
                .with_static_call(true)
                .execute_returns_raw(abi_word(U256::zero()));
        });
    }
}
