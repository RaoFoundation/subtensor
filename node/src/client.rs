use node_subtensor_runtime::{RuntimeApi, opaque::Block};
use polkadot_sdk::cumulus_primitives_proof_size_hostfunction::storage_proof_size::HostFunctions as ProofSize;
use sc_executor::WasmExecutor;

mod legacy_bls12_381 {
    use ark_ec::short_weierstrass::SWCurveConfig;
    use ark_scale_v4::{
        ark_serialize::{Compress, Validate},
        scale::{Decode, Encode},
    };
    use sp_runtime_interface::{
        pass_by::{AllocateAndReturnByCodec, PassFatPointerAndRead},
        runtime_interface,
    };

    const SCALE_USAGE: u8 = ark_scale_v4::make_usage(Compress::No, Validate::No);
    type ArkScale<T> = ark_scale_v4::ArkScale<T, SCALE_USAGE>;
    type ArkScaleProjective<T> = ark_scale_v4::hazmat::ArkScaleProjective<T>;

    fn mul_projective<T: SWCurveConfig>(base: Vec<u8>, scalar: Vec<u8>) -> Result<Vec<u8>, ()> {
        let base = ArkScaleProjective::decode(&mut &base[..])
            .map_err(|_| ())?
            .0;
        let scalar = ArkScale::<Vec<u64>>::decode(&mut &scalar[..])
            .map_err(|_| ())?
            .0;
        let result = T::mul_projective(&base, &scalar);

        Ok(ArkScaleProjective::from(&result).encode())
    }

    /// Legacy BLS12-381 host calls required by runtimes built before stable2606.
    #[allow(dead_code)]
    #[runtime_interface]
    pub trait HostCalls {
        /// Multiply a legacy-encoded BLS12-381 G1 projective point.
        fn bls12_381_mul_projective_g1(
            base: PassFatPointerAndRead<Vec<u8>>,
            scalar: PassFatPointerAndRead<Vec<u8>>,
        ) -> AllocateAndReturnByCodec<Result<Vec<u8>, ()>> {
            mul_projective::<ark_bls12_381::g1::Config>(base, scalar)
        }

        /// Multiply a legacy-encoded BLS12-381 G2 projective point.
        fn bls12_381_mul_projective_g2(
            base: PassFatPointerAndRead<Vec<u8>>,
            scalar: PassFatPointerAndRead<Vec<u8>>,
        ) -> AllocateAndReturnByCodec<Result<Vec<u8>, ()>> {
            mul_projective::<ark_bls12_381::g2::Config>(base, scalar)
        }
    }
}

/// Full backend.
pub type FullBackend = sc_service::TFullBackend<Block>;
/// Full client.
pub type FullClient = sc_service::TFullClient<Block, RuntimeApi, RuntimeExecutor>;
/// Always enable runtime benchmark host functions, the genesis state
/// was built with them so we're stuck with them forever.
///
/// They're just a noop, never actually get used if the runtime was not compiled with
/// `runtime-benchmarks`.
pub type HostFunctions = (
    sp_io::SubstrateHostFunctions,
    frame_benchmarking::benchmarking::HostFunctions,
    sp_crypto_ec_utils::bls12_381::host_calls::HostFunctions,
    legacy_bls12_381::host_calls::HostFunctions,
    ProofSize,
);
pub type RuntimeExecutor = WasmExecutor<HostFunctions>;

#[cfg(test)]
mod tests {
    use super::HostFunctions;
    use sp_runtime_interface::sp_wasm_interface::HostFunctions as _;

    #[test]
    fn registers_legacy_bls12_381_projective_multiplication_host_calls() {
        let names = HostFunctions::host_functions()
            .into_iter()
            .map(|function| function.name())
            .collect::<Vec<_>>();

        for name in [
            "ext_host_calls_bls12_381_mul_projective_g1_version_1",
            "ext_host_calls_bls12_381_mul_projective_g2_version_1",
        ] {
            assert_eq!(
                names
                    .iter()
                    .filter(|registered| **registered == name)
                    .count(),
                1
            );
        }
    }
}
