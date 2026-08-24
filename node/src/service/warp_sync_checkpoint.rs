use crate::chain_spec::{GrandpaWarpSyncCheckpoint, GrandpaWarpSyncCheckpointExtension};
use crate::client::FullClient;
use jsonrpsee::RpcModule;
use jsonrpsee::types::ErrorObjectOwned;
use node_subtensor_runtime::opaque::{Block, Header};
use sc_client_api::BlockBackend;
use sc_consensus_grandpa::{AuthoritySetChanges, AuthoritySetHardFork, SharedAuthoritySet};
use sc_service::error::Error as ServiceError;
use sp_api::ProvideRuntimeApi;
use sp_consensus_grandpa::{AuthorityList, GRANDPA_ENGINE_ID, GrandpaApi, SetId};
use sp_runtime::codec::{DecodeAll, Encode};
use sp_runtime::traits::Header as _;
use std::sync::Arc;

const FINALIZED_BLOCK_HEADER: &str = "finalizedBlockHeader";
const GRANDPA_AUTHORITY_SET: &str = "grandpaAuthoritySet";

/// Read and validate a historical GRANDPA signing checkpoint from the chain-spec extension.
pub(super) fn trusted_checkpoint(
    chain_spec: &dyn sc_chain_spec::ChainSpec,
) -> Result<Option<AuthoritySetHardFork<Block>>, ServiceError> {
    let Some(checkpoint) =
        sc_chain_spec::get_extension::<GrandpaWarpSyncCheckpointExtension>(chain_spec.extensions())
    else {
        return Ok(None);
    };
    let Some(checkpoint) = checkpoint else {
        return Ok(None);
    };

    let header = decode_finalized_header(checkpoint)?;
    let (set_id, authorities) = decode_authority_set(checkpoint)?;
    validate_authorities(&authorities)?;

    Ok(Some(AuthoritySetHardFork {
        set_id,
        block: (header.hash(), *header.number()),
        authorities,
        last_finalized: None,
    }))
}

struct WarpSyncCheckpointRpcContext {
    chain_spec: Box<dyn sc_chain_spec::ChainSpec>,
    client: Arc<FullClient>,
    authority_set: SharedAuthoritySet<sp_core::H256, u32>,
}

/// Expose a GRANDPA-only generator for a historical signing checkpoint.
pub(super) fn rpc_methods(
    chain_spec: Box<dyn sc_chain_spec::ChainSpec>,
    client: Arc<FullClient>,
    authority_set: SharedAuthoritySet<sp_core::H256, u32>,
) -> Result<jsonrpsee::Methods, ServiceError> {
    let mut module = RpcModule::new(WarpSyncCheckpointRpcContext {
        chain_spec,
        client,
        authority_set,
    });
    module
        .register_method("grandpa_genWarpSyncSpec", |params, context, _| {
            let raw = params.one::<bool>()?;
            generate_warp_sync_spec(context, raw).map_err(internal_rpc_error)
        })
        .map_err(|error| ServiceError::Other(error.to_string()))?;
    Ok(module.into())
}

fn generate_warp_sync_spec(
    context: &WarpSyncCheckpointRpcContext,
    raw: bool,
) -> Result<serde_json::Value, ServiceError> {
    let changes = authority_change_records(&context.authority_set.authority_set_changes())?;
    let (set_id, block_number) = changes.iter().last().copied().ok_or_else(|| {
        ServiceError::Other("GRANDPA has no finalized authority transition".into())
    })?;

    let transition_hash = context
        .client
        .block_hash(block_number)
        .map_err(|error| ServiceError::Other(error.to_string()))?
        .ok_or_else(|| {
            ServiceError::Other(format!(
                "authority transition block #{block_number} is missing from the local database"
            ))
        })?;
    let header = context
        .client
        .header(transition_hash)
        .map_err(|error| ServiceError::Other(error.to_string()))?
        .ok_or_else(|| {
            ServiceError::Other(format!(
                "authority transition header {transition_hash:?} is missing from the local database"
            ))
        })?;
    if sc_consensus_grandpa::find_scheduled_change::<Block>(&header).is_none() {
        return Err(ServiceError::Other(format!(
            "authority transition at #{block_number} is not a scheduled GRANDPA change"
        )));
    }
    let has_grandpa_justification = context
        .client
        .justifications(transition_hash)
        .map_err(|error| ServiceError::Other(error.to_string()))?
        .and_then(|justifications| justifications.into_justification(GRANDPA_ENGINE_ID))
        .is_some();
    if !has_grandpa_justification {
        return Err(ServiceError::Other(format!(
            "authority transition at #{block_number} has no retained GRANDPA justification"
        )));
    }

    let authorities = context
        .client
        .runtime_api()
        .grandpa_authorities(*header.parent_hash())
        .map_err(|error| ServiceError::Other(error.to_string()))?;
    validate_authorities(&authorities)?;

    let current_set_id = context.authority_set.set_id();
    if set_id.checked_add(1) != Some(current_set_id) {
        return Err(ServiceError::Other(format!(
            "latest recorded GRANDPA transition ends set {set_id}, but the current set is {current_set_id}"
        )));
    }

    let mut chain_spec = context.chain_spec.cloned_box();
    let checkpoint = sc_chain_spec::get_extension_mut::<GrandpaWarpSyncCheckpointExtension>(
        chain_spec.extensions_mut(),
    )
    .ok_or_else(|| {
        ServiceError::Other("chain spec has no grandpaWarpSyncCheckpoint extension".into())
    })?;
    *checkpoint = Some(GrandpaWarpSyncCheckpoint {
        finalized_block_header: format!("0x{}", hex::encode(header.encode())),
        grandpa_authority_set: format!("0x{}", hex::encode((set_id, authorities).encode())),
    });

    let json = chain_spec
        .as_json(raw)
        .map_err(|error| ServiceError::Other(error.to_string()))?;
    serde_json::from_str(&json).map_err(|error| ServiceError::Other(error.to_string()))
}

fn authority_change_records(
    changes: &AuthoritySetChanges<u32>,
) -> Result<Vec<(SetId, u32)>, ServiceError> {
    // `AuthoritySetChanges` is a SCALE newtype around this vector, but its public iterator
    // deliberately rejects histories whose first retained set is non-zero. Finney legitimately
    // begins at a corrected non-zero set ID, so decode the complete retained record here.
    Vec::<(SetId, u32)>::decode_all(&mut changes.encode().as_slice())
        .map_err(|error| ServiceError::Other(error.to_string()))
}

fn decode_authority_set(
    checkpoint: &GrandpaWarpSyncCheckpoint,
) -> Result<(SetId, AuthorityList), ServiceError> {
    let encoded = &checkpoint.grandpa_authority_set;
    let encoded = encoded.strip_prefix("0x").ok_or_else(|| {
        ServiceError::Other(format!(
            "grandpaWarpSyncCheckpoint.{GRANDPA_AUTHORITY_SET} must start with 0x"
        ))
    })?;
    let bytes = hex::decode(encoded).map_err(|error| {
        ServiceError::Other(format!(
            "invalid grandpaWarpSyncCheckpoint.{GRANDPA_AUTHORITY_SET} hex: {error}"
        ))
    })?;

    <(SetId, AuthorityList)>::decode_all(&mut bytes.as_slice()).map_err(|error| {
        ServiceError::Other(format!(
            "invalid grandpaWarpSyncCheckpoint.{GRANDPA_AUTHORITY_SET}: {error}"
        ))
    })
}

fn validate_authorities(authorities: &AuthorityList) -> Result<(), ServiceError> {
    if authorities.is_empty() || authorities.iter().any(|(_, weight)| *weight == 0) {
        return Err(ServiceError::Other(
            "trusted GRANDPA authority set must be non-empty with non-zero weights".into(),
        ));
    }
    Ok(())
}

fn internal_rpc_error(error: ServiceError) -> ErrorObjectOwned {
    ErrorObjectOwned::owned(-32603, error.to_string(), None::<()>)
}

fn decode_finalized_header(checkpoint: &GrandpaWarpSyncCheckpoint) -> Result<Header, ServiceError> {
    let encoded_header = &checkpoint.finalized_block_header;
    let encoded_header = encoded_header.strip_prefix("0x").ok_or_else(|| {
        ServiceError::Other(format!(
            "grandpaWarpSyncCheckpoint.{FINALIZED_BLOCK_HEADER} must start with 0x"
        ))
    })?;
    let bytes = hex::decode(encoded_header).map_err(|error| {
        ServiceError::Other(format!(
            "invalid grandpaWarpSyncCheckpoint.{FINALIZED_BLOCK_HEADER} hex: {error}"
        ))
    })?;

    Header::decode_all(&mut bytes.as_slice()).map_err(|error| {
        ServiceError::Other(format!(
            "invalid grandpaWarpSyncCheckpoint.{FINALIZED_BLOCK_HEADER} header: {error}"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sp_core::H256;
    use sp_runtime::codec::Encode;

    #[test]
    fn decodes_trusted_checkpoint_header() {
        let header = Header::new(
            42,
            H256::repeat_byte(1),
            H256::repeat_byte(2),
            H256::repeat_byte(3),
            Default::default(),
        );
        let state = GrandpaWarpSyncCheckpoint {
            finalized_block_header: format!("0x{}", hex::encode(header.encode())),
            grandpa_authority_set: "0x".into(),
        };

        assert_eq!(decode_finalized_header(&state).unwrap(), header);
    }

    #[test]
    fn decodes_trusted_checkpoint_authorities() {
        let authorities = vec![(
            sp_consensus_grandpa::AuthorityId::from(sp_core::ed25519::Public::from_raw([7; 32])),
            1,
        )];
        let state = GrandpaWarpSyncCheckpoint {
            finalized_block_header: "0x".into(),
            grandpa_authority_set: format!(
                "0x{}",
                hex::encode((6_u64, authorities.clone()).encode()),
            ),
        };

        assert_eq!(decode_authority_set(&state).unwrap(), (6, authorities));
    }

    #[test]
    fn reads_authority_history_with_a_non_zero_initial_set() {
        let changes = AuthoritySetChanges::from(vec![(3, 10), (4, 20), (5, 30)]);
        assert_eq!(
            authority_change_records(&changes).unwrap(),
            vec![(3, 10), (4, 20), (5, 30)],
        );
    }

    #[test]
    fn finney_chain_spec_contains_the_verified_transition_checkpoint() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../chainspecs/raw_spec_finney.json");
        let spec = crate::chain_spec::ChainSpec::from_json_file(path).unwrap();
        let checkpoint = trusted_checkpoint(&spec).unwrap().unwrap();

        assert_eq!(checkpoint.set_id, 5);
        assert_eq!(checkpoint.block.1, 8_867_448);
        assert_eq!(
            checkpoint.block.0,
            H256::from(hex_literal::hex!(
                "511948e96e1d479d0a92d89bb976638780f2c65a93a5d5be710f22ee15c60200"
            )),
        );
        assert_eq!(checkpoint.authorities.len(), 20);
    }

    #[test]
    fn rejects_missing_checkpoint_fields() {
        let error = serde_json::from_value::<GrandpaWarpSyncCheckpoint>(serde_json::json!({
            GRANDPA_AUTHORITY_SET: "0x"
        }))
        .unwrap_err();
        assert!(error.to_string().contains(FINALIZED_BLOCK_HEADER));

        let error = serde_json::from_value::<GrandpaWarpSyncCheckpoint>(serde_json::json!({
            FINALIZED_BLOCK_HEADER: "0x"
        }))
        .unwrap_err();
        assert!(error.to_string().contains(GRANDPA_AUTHORITY_SET));
    }

    #[test]
    fn rejects_non_hex_header() {
        let state = GrandpaWarpSyncCheckpoint {
            finalized_block_header: "not-hex".into(),
            grandpa_authority_set: "0x".into(),
        };
        assert!(decode_finalized_header(&state).is_err());

        let state = GrandpaWarpSyncCheckpoint {
            finalized_block_header: "0xzz".into(),
            grandpa_authority_set: "0x".into(),
        };
        assert!(decode_finalized_header(&state).is_err());
    }

    #[test]
    fn rejects_trailing_scale_data() {
        let header = Header::new(
            42,
            H256::repeat_byte(1),
            H256::repeat_byte(2),
            H256::repeat_byte(3),
            Default::default(),
        );
        let mut encoded = header.encode();
        encoded.push(0);
        let state = GrandpaWarpSyncCheckpoint {
            finalized_block_header: format!("0x{}", hex::encode(encoded)),
            grandpa_authority_set: "0x".into(),
        };

        assert!(decode_finalized_header(&state).is_err());

        let mut encoded_authorities = (6_u64, AuthorityList::new()).encode();
        encoded_authorities.push(0);
        let state = GrandpaWarpSyncCheckpoint {
            finalized_block_header: "0x".into(),
            grandpa_authority_set: format!("0x{}", hex::encode(encoded_authorities)),
        };
        assert!(decode_authority_set(&state).is_err());
    }

    #[test]
    fn rejects_invalid_authority_lists() {
        assert!(validate_authorities(&AuthorityList::new()).is_err());

        let authorities = vec![(
            sp_consensus_grandpa::AuthorityId::from(sp_core::ed25519::Public::from_raw([7; 32])),
            0,
        )];
        assert!(validate_authorities(&authorities).is_err());
    }
}
