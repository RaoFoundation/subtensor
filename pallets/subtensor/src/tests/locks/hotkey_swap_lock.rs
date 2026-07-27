#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]
//! Hotkey swap interaction.

use super::helpers::*;
use super::prelude::*;

// =========================================================================
// GROUP 12: Hotkey swap interaction
// =========================================================================

#[test]
fn test_hotkey_swap_swaps_locks_and_convictions() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let old_hotkey = U256::from(2);
        let new_hotkey = U256::from(20);
        let netuid = setup_subnet_with_stake(coldkey, old_hotkey, 100_000_000_000);
        Owner::<Test>::insert(old_hotkey, coldkey);
        Owner::<Test>::insert(new_hotkey, coldkey);

        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &old_hotkey,
            5000u64.into(),
        ));
        assert!(LockingColdkeys::<Test>::contains_key((
            netuid, old_hotkey, coldkey
        )));
        assert_eq!(
            LockingColdkeys::<Test>::iter_prefix((netuid, old_hotkey)).count(),
            1
        );

        // Mock a non-zero conviction
        let mut lock = Lock::<Test>::get((coldkey, netuid, old_hotkey)).unwrap();
        lock.conviction = U64F64::from_num(1234);
        Lock::<Test>::insert((coldkey, netuid, old_hotkey), lock);
        let mut hotkey_lock = HotkeyLock::<Test>::get(netuid, old_hotkey).unwrap();
        hotkey_lock.conviction = U64F64::from_num(1234);
        HotkeyLock::<Test>::insert(netuid, old_hotkey, hotkey_lock);

        // Perform hotkey swap
        let mut weight = Weight::zero();
        assert_ok!(SubtensorModule::perform_hotkey_swap_on_all_subnets(
            &old_hotkey,
            &new_hotkey,
            &coldkey,
            &mut weight,
            false
        ));

        // Lock references new_hotkey, conviction is not reset
        let lock = Lock::<Test>::get((coldkey, netuid, new_hotkey)).unwrap();
        assert_eq!(lock.locked_mass, 5000u64.into());
        assert!(lock.conviction > U64F64::from_num(0));
        assert!(!LockingColdkeys::<Test>::contains_key((
            netuid, old_hotkey, coldkey
        )));
        assert!(LockingColdkeys::<Test>::contains_key((
            netuid, new_hotkey, coldkey
        )));

        // Hotkey lock data also updated, conviction is not reset
        let hotkey_lock = HotkeyLock::<Test>::get(netuid, new_hotkey).unwrap();
        assert_eq!(hotkey_lock.locked_mass, 5000u64.into());
        assert!(hotkey_lock.conviction > U64F64::from_num(0));

        // Trying to top up to new_hotkey works
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &new_hotkey,
            100u64.into()
        ));

        // Trying to top up to old_hotkey fails (old_hotkey is no longer associated with coldkey)
        assert_noop!(
            SubtensorModule::do_lock_stake(&coldkey, netuid, &old_hotkey, 100u64.into()),
            Error::<Test>::HotKeyAccountNotExists
        );
    });
}
