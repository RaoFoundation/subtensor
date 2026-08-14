use node_subtensor_runtime::opaque::Block;
use sc_chain_spec::ChainType;
use sc_consensus_grandpa::warp_proof::{HardForks, WarpSyncCheckpoint};
use sc_network_sync::strategy::warp::{EncodedProof, VerificationResult, WarpSyncProvider};
use sp_consensus_grandpa::{AuthorityId, AuthorityList, SetId};
use sp_core::{ByteArray, H256};

const FINNEY_GENESIS: H256 = H256(hex_literal::hex!(
    "2f0555cc76fc2840a25a6ea3b9637146806f1f44b090c175ffde2a7e5ab36c03"
));
const TESTNET_GENESIS: H256 = H256(hex_literal::hex!(
    "8f9cf856bf558a14440e75569c9e58594757048d7b3a84b5d25f6bd978263105"
));

pub(super) enum Config {
    TestnetCheckpoints(Vec<WarpSyncCheckpoint<Block>>),
    OneTimeInitialSetId(SetId),
    InitialSetId(u64),
}

pub(super) fn config(genesis_hash: H256, chain_type: ChainType) -> Config {
    if genesis_hash == TESTNET_GENESIS {
        Config::TestnetCheckpoints(testnet_checkpoints())
    } else if genesis_hash == FINNEY_GENESIS {
        Config::OneTimeInitialSetId(3)
    } else {
        let set_id = match chain_type {
            ChainType::Live => 3,
            ChainType::Development => 2,
            _ => 0,
        };
        Config::InitialSetId(set_id)
    }
}

impl Config {
    pub(super) fn log_message(&self) -> String {
        match self {
            Self::TestnetCheckpoints(_) => "Testnet GRANDPA warp sync checkpoints enabled.".into(),
            Self::OneTimeInitialSetId(set_id) => {
                format!(
                    "Finney GRANDPA warp sync one-time initial set ID patch enabled. Set ID = \
                     {set_id}"
                )
            }
            Self::InitialSetId(set_id) => {
                format!("GRANDPA warp sync initial set ID patch enabled. Set ID = {set_id}")
            }
        }
    }

    pub(super) fn one_time_initial_set_id(&self) -> Option<SetId> {
        match self {
            Self::OneTimeInitialSetId(set_id) => Some(*set_id),
            Self::TestnetCheckpoints(_) | Self::InitialSetId(_) => None,
        }
    }

    pub(super) fn into_hard_forks(self) -> HardForks<Block> {
        match self {
            Self::TestnetCheckpoints(checkpoints) => {
                HardForks::new_authority_set_checkpoints(checkpoints)
            }
            // Keep the provider in reinitialized-set mode so a completed proof does not replace
            // its shared authority set. The outer provider supplies the actual one-time offset.
            Self::OneTimeInitialSetId(_) => HardForks::new_initial_set_id(0),
            Self::InitialSetId(set_id) => HardForks::new_initial_set_id(set_id),
        }
    }
}

pub(super) struct InitialSetIdProvider<P> {
    inner: P,
    initial_set_id: SetId,
}

impl<P> InitialSetIdProvider<P> {
    pub(super) fn new(inner: P, initial_set_id: SetId) -> Self {
        Self {
            inner,
            initial_set_id,
        }
    }
}

impl<P> WarpSyncProvider<Block> for InitialSetIdProvider<P>
where
    P: WarpSyncProvider<Block>,
{
    fn generate(
        &self,
        start: H256,
    ) -> Result<EncodedProof, Box<dyn std::error::Error + Send + Sync>> {
        self.inner.generate(start)
    }

    fn verify(
        &self,
        proof: &EncodedProof,
        set_id: SetId,
        authorities: AuthorityList,
    ) -> Result<VerificationResult<Block>, Box<dyn std::error::Error + Send + Sync>> {
        // Warp sync starts from set 0. Apply Finney's historical correction there, then trust the
        // set ID returned by each proof. The SDK's `new_initial_set_id` applies its correction to
        // every fragment, which over-counts as soon as a proof spans new rotations.
        let set_id = if set_id == 0 {
            self.initial_set_id
        } else {
            set_id
        };
        self.inner.verify(proof, set_id, authorities)
    }

    fn current_authorities(&self) -> AuthorityList {
        self.inner.current_authorities()
    }
}

#[allow(clippy::expect_used)]
fn testnet_checkpoint_authorities() -> AuthorityList {
    [
        hex_literal::hex!("dc832c3b7bdfc721e90e5ee9e532c06b62a0def3c79dab5324460d938db6600a"),
        hex_literal::hex!("c8a00ef71912b3868b101cb70ebd029999d1c9b6a1390122a98f60d72b9a0fc4"),
        hex_literal::hex!("ee70f7b52998c2b4f3d42e509e8360cda92b0cd4ca100cd4d32be5a1ac297909"),
        hex_literal::hex!("b57a038c9139a060358f3b654df74a1cb6d15bcdb8438bcebd64ce67ec4301eb"),
        hex_literal::hex!("755f75dfc66aaa3b1e761a8845249509b8bd2fdf0d94cb74e1e12e1e0f4d3519"),
        hex_literal::hex!("d97a64267f177505b0565a18677c9f5d4284d7f2eb96d515556e7e52217f82e9"),
    ]
    .into_iter()
    .map(|bytes| {
        (
            AuthorityId::from_slice(&bytes).expect("authority IDs are exactly 32 bytes"),
            1,
        )
    })
    .collect()
}

fn testnet_checkpoints() -> Vec<WarpSyncCheckpoint<Block>> {
    let authorities = testnet_checkpoint_authorities();

    [
        (
            1,
            4_589_686,
            hex_literal::hex!("2b001bfdec34d007ab2ac07f712e64d0cb1a6fb4b51f7d47bfb3c7d7336a689b"),
            None,
        ),
        (
            2,
            5_534_451,
            hex_literal::hex!("4d643da5fd7cd2b9ceb795091643e7223819e2a01f942ac049c5b928f7e30dc4"),
            Some(2),
        ),
    ]
    .into_iter()
    .map(
        |(set_id, number, hash, resulting_set_id)| WarpSyncCheckpoint {
            set_id,
            block: (H256::from(hash), number),
            authorities: authorities.clone(),
            resulting_set_id,
        },
    )
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[derive(Default)]
    struct RecordingProvider {
        set_id: AtomicU64,
    }

    impl WarpSyncProvider<Block> for RecordingProvider {
        fn generate(
            &self,
            _start: H256,
        ) -> Result<EncodedProof, Box<dyn std::error::Error + Send + Sync>> {
            Ok(EncodedProof(Vec::new()))
        }

        fn verify(
            &self,
            _proof: &EncodedProof,
            set_id: SetId,
            authorities: AuthorityList,
        ) -> Result<VerificationResult<Block>, Box<dyn std::error::Error + Send + Sync>> {
            self.set_id.store(set_id, Ordering::Relaxed);
            Ok(VerificationResult::Partial(
                set_id.saturating_add(1),
                authorities,
                H256::zero(),
            ))
        }

        fn current_authorities(&self) -> AuthorityList {
            Vec::new()
        }
    }

    #[test]
    fn checkpoints_are_exactly_testnet_genesis_scoped() {
        assert!(matches!(
            config(TESTNET_GENESIS, ChainType::Live),
            Config::TestnetCheckpoints(_)
        ));
        assert!(matches!(
            config(FINNEY_GENESIS, ChainType::Live),
            Config::OneTimeInitialSetId(3)
        ));
        assert!(matches!(
            config(H256::zero(), ChainType::Live),
            Config::InitialSetId(3)
        ));
        assert!(matches!(
            config(H256::zero(), ChainType::Development),
            Config::InitialSetId(2)
        ));
        assert!(matches!(
            config(H256::zero(), ChainType::Local),
            Config::InitialSetId(0)
        ));
    }

    #[test]
    fn testnet_checkpoints_use_historical_signing_set_and_transition_override() {
        let checkpoints = testnet_checkpoints();
        let [first, second] = checkpoints.as_slice() else {
            panic!("expected exactly two testnet warp checkpoints");
        };

        assert_eq!((first.set_id, first.block.1), (1, 4_589_686));
        assert_eq!(
            first.block.0,
            H256::from(hex_literal::hex!(
                "2b001bfdec34d007ab2ac07f712e64d0cb1a6fb4b51f7d47bfb3c7d7336a689b"
            ))
        );
        assert_eq!((second.set_id, second.block.1), (2, 5_534_451));
        assert_eq!(
            second.block.0,
            H256::from(hex_literal::hex!(
                "4d643da5fd7cd2b9ceb795091643e7223819e2a01f942ac049c5b928f7e30dc4"
            ))
        );
        assert_eq!(first.resulting_set_id, None);
        assert_eq!(second.resulting_set_id, Some(2));
        assert_eq!(first.authorities.len(), 6);
        assert_eq!(first.authorities, second.authorities);
        let authority_ids: Vec<&[u8]> = first
            .authorities
            .iter()
            .map(|(authority_id, _)| AsRef::<[u8]>::as_ref(authority_id))
            .collect();
        let expected_authority_ids = [
            hex_literal::hex!("dc832c3b7bdfc721e90e5ee9e532c06b62a0def3c79dab5324460d938db6600a"),
            hex_literal::hex!("c8a00ef71912b3868b101cb70ebd029999d1c9b6a1390122a98f60d72b9a0fc4"),
            hex_literal::hex!("ee70f7b52998c2b4f3d42e509e8360cda92b0cd4ca100cd4d32be5a1ac297909"),
            hex_literal::hex!("b57a038c9139a060358f3b654df74a1cb6d15bcdb8438bcebd64ce67ec4301eb"),
            hex_literal::hex!("755f75dfc66aaa3b1e761a8845249509b8bd2fdf0d94cb74e1e12e1e0f4d3519"),
            hex_literal::hex!("d97a64267f177505b0565a18677c9f5d4284d7f2eb96d515556e7e52217f82e9"),
        ];
        let expected_authority_ids = expected_authority_ids
            .iter()
            .map(|id| id.as_slice())
            .collect::<Vec<_>>();
        assert_eq!(authority_ids, expected_authority_ids);
    }

    #[test]
    fn finney_initial_set_id_is_applied_only_at_warp_sync_start() {
        let provider = InitialSetIdProvider::new(RecordingProvider::default(), 3);
        let proof = EncodedProof(Vec::new());

        let Ok(first) = provider.verify(&proof, 0, Vec::new()) else {
            panic!("first proof should verify");
        };
        assert!(matches!(first, VerificationResult::Partial(4, _, _)));
        assert_eq!(provider.inner.set_id.load(Ordering::Relaxed), 3);

        let Ok(second) = provider.verify(&proof, 4, Vec::new()) else {
            panic!("second proof should verify");
        };
        assert!(matches!(second, VerificationResult::Partial(5, _, _)));
        assert_eq!(provider.inner.set_id.load(Ordering::Relaxed), 4);
    }
}
