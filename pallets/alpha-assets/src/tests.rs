#![allow(clippy::unwrap_used)]

use frame_support::traits::{Imbalance, tokens::imbalance::TryMerge};
use subtensor_runtime_common::Token;
use subtensor_runtime_common::{AlphaBalance, NetUid};

use crate::{
    AlphaAssetsInterface, AlphaBurned, AlphaRecycled, PositiveAlphaImbalance, TotalAlphaIssuance,
};

use super::mock::*;

#[test]
fn mint_alpha_increases_total_issuance_and_returns_imbalance() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(3u16);
        let amount = AlphaBalance::from(75u64);

        let minted = AlphaAssets::mint_alpha(netuid, amount);

        assert_eq!(TotalAlphaIssuance::<Test>::get(netuid), amount);
        assert_eq!(minted, PositiveAlphaImbalance::new(netuid, amount));
        assert_eq!(minted.netuid(), netuid);
        assert_eq!(minted.amount(), amount);
    });
}

#[test]
fn burn_alpha_does_not_change_total_issuance() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(4u16);
        let minted = AlphaAssets::mint_alpha(netuid, 100u64.into());

        let burned = AlphaAssets::burn_alpha(netuid, 40u64.into());

        assert_eq!(minted.amount(), 100u64.into());
        assert_eq!(burned, 40u64.into());
        assert_eq!(AlphaBurned::<Test>::get(netuid), 40u64.into());
        assert_eq!(TotalAlphaIssuance::<Test>::get(netuid), 100u64.into());
    });
}

#[test]
fn recycle_alpha_reduces_total_issuance_saturating_at_zero() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(5u16);

        AlphaAssets::mint_alpha(netuid, 90u64.into());
        let recycled = <AlphaAssets as AlphaAssetsInterface>::recycle_alpha(netuid, 30u64.into());
        assert_eq!(recycled, 30u64.into());
        assert_eq!(AlphaRecycled::<Test>::get(netuid), 30u64.into());
        assert_eq!(TotalAlphaIssuance::<Test>::get(netuid), 60u64.into());

        AlphaAssets::recycle_alpha(netuid, 100u64.into());
        assert_eq!(AlphaRecycled::<Test>::get(netuid), 130u64.into());
        assert_eq!(TotalAlphaIssuance::<Test>::get(netuid), AlphaBalance::ZERO);
    });
}

#[test]
fn positive_imbalance_only_merges_with_same_netuid() {
    new_test_ext().execute_with(|| {
        let netuid_a = NetUid::from(1u16);
        let netuid_b = NetUid::from(2u16);

        let merged = PositiveAlphaImbalance::new(netuid_a, 10u64.into())
            .merge(PositiveAlphaImbalance::new(netuid_a, 15u64.into()));
        assert_eq!(merged.peek(), 25u64.into());

        let merge_result = TryMerge::try_merge(
            PositiveAlphaImbalance::new(netuid_a, 10u64.into()),
            PositiveAlphaImbalance::new(netuid_b, 15u64.into()),
        );
        assert!(merge_result.is_err());
    });
}
