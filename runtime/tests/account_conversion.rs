#![allow(clippy::unwrap_used)]

use node_subtensor_runtime::{BuildStorage, RuntimeGenesisConfig, SubtensorModule, System};
use subtensor_runtime_common::NetUid;

fn new_test_ext() -> sp_io::TestExternalities {
    sp_tracing::try_init_simple();
    let mut ext: sp_io::TestExternalities = RuntimeGenesisConfig {
        balances: pallet_balances::GenesisConfig {
            balances: vec![],
            dev_accounts: None,
        },
        ..Default::default()
    }
    .build_storage()
    .unwrap()
    .into();
    ext.execute_with(|| System::set_block_number(1));
    ext
}

/// Test full-range netuids on real ss58 accounts to ensure no panics
/// cargo test --package node-subtensor-runtime --test account_conversion -- test_subnet_account_id_no_panics --exact --nocapture
#[test]
#[ignore]
fn test_subnet_account_id_no_panics() {
    new_test_ext().execute_with(|| {
        for raw_netuid in 0u16..=u16::MAX {
            let netuid = NetUid::from(raw_netuid);
            SubtensorModule::init_new_network(netuid, 10);

            let account_id = SubtensorModule::get_subnet_account_id(netuid).unwrap();
            let roudtrip_netuid = SubtensorModule::is_subnet_account_id(&account_id);
            assert_eq!(netuid, roudtrip_netuid.unwrap());
        }
    });
}

/// Quick sanity test
/// cargo test --package node-subtensor-runtime --test account_conversion -- test_subnet_account_id_no_panics_quick --exact --nocapture
#[test]
fn test_subnet_account_id_no_panics_quick() {
    new_test_ext().execute_with(|| {
        for raw_netuid in 0u16..=1024u16 {
            let netuid = NetUid::from(raw_netuid);
            SubtensorModule::init_new_network(netuid, 10);

            let account_id = SubtensorModule::get_subnet_account_id(netuid).unwrap();
            let roudtrip_netuid = SubtensorModule::is_subnet_account_id(&account_id);
            assert_eq!(netuid, roudtrip_netuid.unwrap());
        }
    });
}
