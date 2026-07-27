//! Public development-network chain-spec (`--chain devnet`).
//!
//! Authorities are fixed SS58 keys for the shared debug validator set.

// Allowed since it's actually better to panic during chain setup when there is an error
#![allow(clippy::unwrap_used)]

use super::*;

/// Build the shared public-devnet `ChainSpec`.
pub fn devnet_config() -> Result<ChainSpec, String> {
    let wasm_binary = WASM_BINARY.ok_or_else(|| "Development wasm not available".to_string())?;

    // Give front-ends necessary data to present to users
    let mut properties = sc_service::Properties::new();
    properties.insert("tokenSymbol".into(), "testTAO".into());
    properties.insert("tokenDecimals".into(), 9.into());
    properties.insert("ss58Format".into(), 42.into());

    Ok(ChainSpec::builder(
        wasm_binary,
        Extensions {
            bad_blocks: Some(HashSet::new()),
            ..Default::default()
        },
    )
    .with_name("Bittensor")
    .with_protocol_id("bittensor")
    .with_id("bittensor")
    .with_chain_type(ChainType::Development)
    .with_genesis_config_patch(devnet_genesis(
        // Initial PoA authorities (Validators)
        // aura | grandpa
        vec![
            // Keys for debug
            authority_keys_from_ss58(
                "5D5ABUyMsdmJdH7xrsz9vREq5eGXr5pXhHxix2dENQR62dEo",
                "5H3qMjQjoeZxZ98jzDmoCwbz2sugd5fDN1wrr8Phf49zemKL",
            ),
            authority_keys_from_ss58(
                "5GbRc5sNDdhcPAU9suV2g9P5zyK1hjAQ9JHeeadY1mb8kXoM",
                "5GbkysfaCjK3cprKPhi3CUwaB5xWpBwcfrkzs6FmqHxej8HZ",
            ),
            authority_keys_from_ss58(
                "5CoVWwBwXz2ndEChGcS46VfSTb3RMUZzZzAYdBKo263zDhEz",
                "5HTLp4BvPp99iXtd8YTBZA1sMfzo8pd4mZzBJf7HYdCn2boU",
            ),
            authority_keys_from_ss58(
                "5EekcbqupwbgWqF8hWGY4Pczsxp9sbarjDehqk7bdyLhDCwC",
                "5GAemcU4Pzyfe8DwLwDFx3aWzyg3FuqYUCCw2h4sdDZhyFvE",
            ),
            authority_keys_from_ss58(
                "5GgdEQyS5DZzUwKuyucEPEZLxFKGmasUFm1mqM3sx1MRC5RV",
                "5EibpMomXmgekxcfs25SzFBpGWUsG9Lc8ALNjXN3TYH5Tube",
            ),
        ],
        // Sudo account
        Ss58Codec::from_ss58check("5GpzQgpiAKHMWNSH3RN4GLf96GVTDct9QxYEFAY7LWcVzTbx").unwrap(),
        // Pre-funded accounts
        vec![],
        true,
        vec![],
        vec![],
        0,
    ))
    .with_properties(properties)
    .build())
}

// Configure initial storage state for FRAME modules.
#[allow(clippy::too_many_arguments)]
fn devnet_genesis(
    initial_authorities: Vec<(AuraId, GrandpaId)>,
    root_key: AccountId,
    _endowed_accounts: Vec<AccountId>,
    _enable_println: bool,
    _stakes: Vec<(AccountId, Vec<(AccountId, (u64, u16))>)>,
    _balances: Vec<(AccountId, u64)>,
    _balances_issuance: u64,
) -> serde_json::Value {
    serde_json::json!({
        "balances": {
            "balances": vec![(root_key.clone(), 1_000_000_000_000u128)],
        },
        "aura": {
            "authorities": initial_authorities.iter().map(|x| (x.0.clone())).collect::<Vec<_>>(),
        },
        "grandpa": {
            "authorities": initial_authorities
                .iter()
                .map(|x| (x.1.clone(), 1))
                .collect::<Vec<_>>(),
        },
        "sudo": {
            "key": Some(root_key),
        },
    })
}
