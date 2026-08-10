use node_subtensor_runtime::opaque::Block;
use sc_chain_spec::ChainType;
use sc_consensus_grandpa::{AuthoritySetHardFork, warp_proof::HardForks};
use sp_consensus_grandpa::{AuthorityId, AuthorityList};
use sp_core::{ByteArray, H256};

const TESTNET_GENESIS: H256 = H256(hex_literal::hex!(
    "8f9cf856bf558a14440e75569c9e58594757048d7b3a84b5d25f6bd978263105"
));

pub(super) enum Config {
    TestnetCheckpoints(Vec<AuthoritySetHardFork<Block>>),
    InitialSetId(u64),
}

pub(super) fn config(genesis_hash: H256, chain_type: ChainType) -> Config {
    if genesis_hash == TESTNET_GENESIS {
        Config::TestnetCheckpoints(testnet_checkpoints())
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
            Self::InitialSetId(set_id) => {
                format!("GRANDPA warp sync initial set ID patch enabled. Set ID = {set_id}")
            }
        }
    }

    pub(super) fn into_hard_forks(self) -> HardForks<Block> {
        match self {
            Self::TestnetCheckpoints(checkpoints) => {
                HardForks::new_hard_forked_authorities(checkpoints)
            }
            Self::InitialSetId(set_id) => HardForks::new_initial_set_id(set_id),
        }
    }
}

#[allow(clippy::expect_used)]
fn testnet_authorities() -> AuthorityList {
    [
        hex_literal::hex!("dc832c3b7bdfc721e90e5ee9e532c06b62a0def3c79dab5324460d938db6600a"),
        hex_literal::hex!("c8a00ef71912b3868b101cb70ebd029999d1c9b6a1390122a98f60d72b9a0fc4"),
        hex_literal::hex!("ee70f7b52998c2b4f3d42e509e8360cda92b0cd4ca100cd4d32be5a1ac297909"),
        hex_literal::hex!("b57a038c9139a060358f3b654df74a1cb6d15bcdb8438bcebd64ce67ec4301eb"),
        hex_literal::hex!("755f75dfc66aaa3b1e761a8845249509b8bd2fdf0d94cb74e1e12e1e0f4d3519"),
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

fn testnet_checkpoints() -> Vec<AuthoritySetHardFork<Block>> {
    let authorities = testnet_authorities();

    [
        (
            1,
            4_589_686,
            hex_literal::hex!("2b001bfdec34d007ab2ac07f712e64d0cb1a6fb4b51f7d47bfb3c7d7336a689b"),
        ),
        (
            2,
            5_534_451,
            hex_literal::hex!("4d643da5fd7cd2b9ceb795091643e7223819e2a01f942ac049c5b928f7e30dc4"),
        ),
    ]
    .into_iter()
    .map(|(set_id, number, hash)| AuthoritySetHardFork {
        set_id,
        block: (H256::from(hash), number),
        authorities: authorities.clone(),
        last_finalized: None,
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkpoints_are_exactly_testnet_genesis_scoped() {
        assert!(matches!(
            config(TESTNET_GENESIS, ChainType::Live),
            Config::TestnetCheckpoints(_)
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
    fn testnet_checkpoints_are_ordered_and_share_authorities() {
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
        assert_eq!(first.authorities.len(), 5);
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
        ];
        let expected_authority_ids = expected_authority_ids
            .iter()
            .map(|id| id.as_slice())
            .collect::<Vec<_>>();
        assert_eq!(authority_ids, expected_authority_ids);
    }
}
