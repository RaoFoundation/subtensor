#![allow(
    unused,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used
)]
//! Root proportion bookkeeping on block step.

use super::helpers::*;
use super::prelude::*;

#[test]
fn test_root_prop_filled_on_block_step() {
    new_test_ext(1).execute_with(|| {
        let hotkey = U256::from(10);
        let coldkey = U256::from(11);
        let netuid1 = add_dynamic_network(&hotkey, &coldkey);
        let netuid2 = add_dynamic_network(&hotkey, &coldkey);

        SubnetTAO::<Test>::insert(NetUid::ROOT, TaoBalance::from(1_000_000_000_000u64));
        SubtensorModule::set_tao_weight(u64::MAX); // Set TAO weight to 1.0

        let tao_reserve = TaoBalance::from(50_000_000_000_u64);
        let alpha_in = AlphaBalance::from(100_000_000_000_u64);
        SubnetTAO::<Test>::insert(netuid1, tao_reserve);
        SubnetAlphaIn::<Test>::insert(netuid1, alpha_in);
        SubnetTAO::<Test>::insert(netuid2, tao_reserve);
        SubnetAlphaIn::<Test>::insert(netuid2, alpha_in);

        assert!(!RootProp::<Test>::contains_key(netuid1));
        assert!(!RootProp::<Test>::contains_key(netuid2));

        run_to_block(2);

        assert!(RootProp::<Test>::get(netuid1) > U96F32::from_num(0));
        assert!(RootProp::<Test>::get(netuid2) > U96F32::from_num(0));
    });
}

#[test]
fn test_root_proportion() {
    new_test_ext(1).execute_with(|| {
        let hotkey = U256::from(10);
        let coldkey = U256::from(11);
        let netuid = add_dynamic_network(&hotkey, &coldkey);

        let root_tao_reserve = 1_000_000_000_000u64;
        SubnetTAO::<Test>::insert(NetUid::ROOT, TaoBalance::from(root_tao_reserve));

        let tao_weight = 3_320_413_933_267_719_290u64;
        SubtensorModule::set_tao_weight(tao_weight);

        let alpha_in = 100_000_000_000u64;
        SubnetAlphaIn::<Test>::insert(netuid, AlphaBalance::from(alpha_in));

        let actual_root_proportion = SubtensorModule::root_proportion(netuid);
        let expected_root_prop = {
            let tao_weight = SubtensorModule::get_tao_weight();
            let root_tao = U96F32::from_num(root_tao_reserve);
            let alpha_in = {
                let alpha: u64 = SubtensorModule::get_alpha_issuance(netuid).into();

                U96F32::from_num(alpha)
            };

            tao_weight * root_tao / (tao_weight * root_tao + alpha_in)
        };

        assert_eq!(actual_root_proportion, expected_root_prop);
    });
}
