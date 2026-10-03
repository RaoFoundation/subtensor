// Allowed since it's actually better to panic during chain setup when there is an error
#![allow(clippy::unwrap_used)]

use super::*;

/// Names a runtime wasm file that replaces the embedded runtime in the `dev` and
/// `local` chain specs. CI uses it to run a prebuilt node with a pull request's
/// runtime when the node's own code is unchanged; `finney`, `test_finney`,
/// `devnet`, and JSON chain specs never read it.
pub const LOCALNET_RUNTIME_WASM_ENV: &str = "SUBTENSOR_LOCALNET_RUNTIME_WASM";

fn localnet_runtime_wasm() -> Result<std::borrow::Cow<'static, [u8]>, String> {
    let Some(path) = env::var_os(LOCALNET_RUNTIME_WASM_ENV) else {
        return WASM_BINARY
            .map(std::borrow::Cow::Borrowed)
            .ok_or_else(|| "Development wasm not available".to_string());
    };
    let path = std::path::PathBuf::from(path);
    let wasm = std::fs::read(&path)
        .map_err(|error| format!("{LOCALNET_RUNTIME_WASM_ENV}={}: {error}", path.display()))?;
    log::info!(
        "Using runtime wasm from {LOCALNET_RUNTIME_WASM_ENV}={} instead of the embedded runtime",
        path.display()
    );
    Ok(std::borrow::Cow::Owned(wasm))
}

pub fn localnet_config(single_authority: bool) -> Result<ChainSpec, String> {
    let wasm_binary = localnet_runtime_wasm()?;

    // Give front-ends necessary data to present to users
    let mut properties = sc_service::Properties::new();
    properties.insert("tokenSymbol".into(), "TAO".into());
    properties.insert("tokenDecimals".into(), 9.into());
    properties.insert("ss58Format".into(), 42.into());

    Ok(ChainSpec::builder(
        &wasm_binary,
        Extensions {
            bad_blocks: Some(HashSet::from_iter(vec![
                // Example bad block
                H256::from_str(
                    "0xc174d485de4bc3813ac249fe078af605c74ff91d07b0a396cf75fa04f81fa312",
                )
                .unwrap(),
            ])),
            ..Default::default()
        },
    )
    .with_name("Bittensor")
    .with_protocol_id("bittensor")
    .with_id("bittensor")
    .with_chain_type(ChainType::Local)
    .with_genesis_config_patch(localnet_genesis(
        // Initial PoA authorities (Validators)
        // aura | grandpa
        if single_authority {
            // single authority allows you to run the network using a single node
            vec![authority_keys_from_seed("One")]
        } else {
            vec![
                authority_keys_from_seed("One"),
                authority_keys_from_seed("Two"),
                authority_keys_from_seed("Three"),
            ]
        },
        // Pre-funded accounts
        true,
    ))
    .with_properties(properties)
    .build())
}

fn localnet_genesis(
    initial_authorities: Vec<(AuraId, GrandpaId)>,
    _enable_println: bool,
) -> serde_json::Value {
    let mut balances = vec![
        (
            get_account_id_from_seed::<sr25519::Public>("Alice"),
            1000000000000000u128,
        ),
        (
            get_account_id_from_seed::<sr25519::Public>("Bob"),
            1000000000000000u128,
        ),
        (
            get_account_id_from_seed::<sr25519::Public>("Charlie"),
            1000000000000000u128,
        ),
        (
            get_account_id_from_seed::<sr25519::Public>("Dave"),
            2000000000000u128,
        ),
        (
            get_account_id_from_seed::<sr25519::Public>("Eve"),
            2000000000000u128,
        ),
        (
            get_account_id_from_seed::<sr25519::Public>("Ferdie"),
            2000000000000u128,
        ),
        (
            get_account_id_from_seed::<sr25519::Public>("One"),
            2000000000000u128,
        ),
        (
            get_account_id_from_seed::<sr25519::Public>("Two"),
            2000000000000u128,
        ),
        (
            get_account_id_from_seed::<sr25519::Public>("Three"),
            2000000000000u128,
        ),
        // ETH
        (
            // Alith - 0xf24FF3a9CF04c71Dbc94D0b566f7A27B94566cac
            AccountId::from_str("5Fghzk1AJt88PeFEzuRfXzbPchiBbsVGTTXcdx599VdZzkTA").unwrap(),
            2000000000000u128,
        ),
        (
            // Baltathar - 0x3Cd0A705a2DC65e5b1E1205896BaA2be8A07c6e0
            AccountId::from_str("5GeqNhKWj1KG78VHzbmo3ZjZgUTrCuWeamdgiA114XHGdaEr").unwrap(),
            2000000000000u128,
        ),
    ];

    // Check if the environment variable is set
    if let Ok(bt_wallet) = env::var("BT_DEFAULT_TOKEN_WALLET") {
        if let Ok(decoded_wallet) = Ss58Codec::from_ss58check(&bt_wallet) {
            if let Some(existing) = balances.iter_mut().find(|(acc, _)| acc == &decoded_wallet) {
                existing.1 = 1_000_000_000_000_000u128;
            } else {
                balances.push((decoded_wallet, 1_000_000_000_000_000u128));
            }
        } else {
            eprintln!("Invalid format for BT_DEFAULT_TOKEN_WALLET.");
        }
    }

    serde_json::json!({
        "balances": { "balances": balances },
        "aura": {
            "authorities": initial_authorities.iter().map(|x| (x.0.clone())).collect::<Vec<_>>()
        },
        "grandpa": {
            "authorities": initial_authorities
                .iter()
                .map(|x| (x.1.clone(), 1))
                .collect::<Vec<_>>()
        },
        "sudo": {
            "key": Some(get_account_id_from_seed::<sr25519::Public>("Alice"))
        },
        "evmChainId": {
            "chainId": 42,
        },
    })
}
