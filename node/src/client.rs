use node_subtensor_runtime::{RuntimeApi, opaque::Block};
use polkadot_sdk::cumulus_primitives_proof_size_hostfunction::storage_proof_size::HostFunctions as ProofSize;
use sc_executor::WasmExecutor;

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
    ProofSize,
);
pub type RuntimeExecutor = WasmExecutor<HostFunctions>;

#[cfg(test)]
mod tests {
    use super::HostFunctions;
    use sc_executor::HostFunctions as _;

    #[test]
    fn registers_deployed_and_current_bls12_381_host_calls() {
        let names = HostFunctions::host_functions()
            .into_iter()
            .map(|function| function.name())
            .collect::<Vec<_>>();

        for name in [
            "ext_host_calls_bls12_381_multi_miller_loop_version_1",
            "ext_host_calls_bls12_381_multi_miller_loop_version_2",
            "ext_host_calls_bls12_381_final_exponentiation_version_1",
            "ext_host_calls_bls12_381_final_exponentiation_version_2",
            "ext_host_calls_bls12_381_msm_g1_version_1",
            "ext_host_calls_bls12_381_msm_g1_version_2",
            "ext_host_calls_bls12_381_msm_g2_version_1",
            "ext_host_calls_bls12_381_msm_g2_version_2",
            "ext_host_calls_bls12_381_mul_projective_g1_version_1",
            "ext_host_calls_bls12_381_mul_projective_g2_version_1",
            "ext_host_calls_bls12_381_mul_g1_version_1",
            "ext_host_calls_bls12_381_mul_g2_version_1",
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
