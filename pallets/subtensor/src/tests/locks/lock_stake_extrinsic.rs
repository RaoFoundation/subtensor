#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]
//! Lock extrinsic via dispatch.

use super::helpers::*;
use super::prelude::*;

// =========================================================================
// GROUP 13: Lock extrinsic via dispatch
// =========================================================================

#[test]
fn test_lock_stake_extrinsic() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);

        let lock_amount: u64 = 5000;
        assert_ok!(SubtensorModule::lock_stake(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid,
            lock_amount.into(),
        ));

        let lock = Lock::<Test>::get((coldkey, netuid, hotkey)).expect("Lock should exist");
        assert_eq!(lock.locked_mass, lock_amount.into());
        assert_eq!(lock.conviction, U64F64::from_num(0));

        // Hotkey lock should also be updated
        let hotkey_lock =
            HotkeyLock::<Test>::get(netuid, hotkey).expect("Hotkey lock should exist");
        assert_eq!(hotkey_lock.locked_mass, lock_amount.into());
        assert_eq!(hotkey_lock.conviction, U64F64::from_num(0));
    });
}
