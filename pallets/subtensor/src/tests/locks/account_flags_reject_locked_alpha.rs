#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]
//! AccountFlags reject-locked-alpha defaults and fee path.

use super::prelude::*;

#[test]
fn test_account_flags_default_to_zero_and_reject_locked_alpha_setter_pays_fee() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);

        assert_eq!(AccountFlags::<Test>::get(coldkey), 0);
        assert!(!AccountFlags::<Test>::contains_key(coldkey));
        assert!(SubtensorModule::account_rejects_locked_alpha(&coldkey));

        let call =
            RuntimeCall::SubtensorModule(crate::Call::set_reject_locked_alpha { enabled: true });
        assert_eq!(call.get_dispatch_info().pays_fee, Pays::Yes);

        assert_ok!(SubtensorModule::set_reject_locked_alpha(
            RuntimeOrigin::signed(coldkey),
            false,
        ));
        assert_eq!(
            AccountFlags::<Test>::get(coldkey),
            ACCOUNT_FLAGS_ACCEPT_LOCKED_ALPHA
        );
        assert!(AccountFlags::<Test>::contains_key(coldkey));
        assert!(!SubtensorModule::account_rejects_locked_alpha(&coldkey));

        assert_ok!(SubtensorModule::set_reject_locked_alpha(
            RuntimeOrigin::signed(coldkey),
            true,
        ));
        assert_eq!(AccountFlags::<Test>::get(coldkey), 0);
        assert!(!AccountFlags::<Test>::contains_key(coldkey));
        assert!(SubtensorModule::account_rejects_locked_alpha(&coldkey));
    });
}
