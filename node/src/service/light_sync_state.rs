use node_subtensor_runtime::opaque::Header;
use sc_service::error::Error as ServiceError;
use sp_runtime::codec::DecodeAll;

const FINALIZED_BLOCK_HEADER: &str = "finalizedBlockHeader";

/// Read and validate the finalized header from the standard `lightSyncState` chain-spec
/// extension. The chain spec is trusted input, so this header can safely replace the warp-proof
/// phase and bootstrap state sync directly.
pub(super) fn trusted_checkpoint(
    chain_spec: &dyn sc_chain_spec::ChainSpec,
) -> Result<Option<Header>, ServiceError> {
    let Some(light_sync_state) = sc_chain_spec::get_extension::<
        sc_sync_state_rpc::LightSyncStateExtension,
    >(chain_spec.extensions()) else {
        return Ok(None);
    };
    let Some(light_sync_state) = light_sync_state else {
        return Ok(None);
    };

    decode_finalized_header(light_sync_state).map(Some)
}

fn decode_finalized_header(light_sync_state: &serde_json::Value) -> Result<Header, ServiceError> {
    let encoded_header = light_sync_state
        .get(FINALIZED_BLOCK_HEADER)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            ServiceError::Other(format!(
                "lightSyncState.{FINALIZED_BLOCK_HEADER} must be a SCALE-encoded hex string"
            ))
        })?;
    let encoded_header = encoded_header.strip_prefix("0x").ok_or_else(|| {
        ServiceError::Other(format!(
            "lightSyncState.{FINALIZED_BLOCK_HEADER} must start with 0x"
        ))
    })?;
    let bytes = hex::decode(encoded_header).map_err(|error| {
        ServiceError::Other(format!(
            "invalid lightSyncState.{FINALIZED_BLOCK_HEADER} hex: {error}"
        ))
    })?;

    Header::decode_all(&mut bytes.as_slice()).map_err(|error| {
        ServiceError::Other(format!(
            "invalid lightSyncState.{FINALIZED_BLOCK_HEADER} header: {error}"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sp_core::H256;
    use sp_runtime::codec::Encode;
    use sp_runtime::traits::Header as _;

    #[test]
    fn decodes_standard_sync_state_header() {
        let header = Header::new(
            42,
            H256::repeat_byte(1),
            H256::repeat_byte(2),
            H256::repeat_byte(3),
            Default::default(),
        );
        let state = serde_json::json!({
            FINALIZED_BLOCK_HEADER: format!("0x{}", hex::encode(header.encode())),
        });

        assert_eq!(decode_finalized_header(&state).unwrap(), header);
    }

    #[test]
    fn rejects_missing_header() {
        let error = decode_finalized_header(&serde_json::json!({})).unwrap_err();
        assert!(error.to_string().contains(FINALIZED_BLOCK_HEADER));
    }

    #[test]
    fn rejects_non_hex_header() {
        let state = serde_json::json!({ FINALIZED_BLOCK_HEADER: "not-hex" });
        assert!(decode_finalized_header(&state).is_err());

        let state = serde_json::json!({ FINALIZED_BLOCK_HEADER: "0xzz" });
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
        let state = serde_json::json!({
            FINALIZED_BLOCK_HEADER: format!("0x{}", hex::encode(encoded)),
        });

        assert!(decode_finalized_header(&state).is_err());
    }
}
