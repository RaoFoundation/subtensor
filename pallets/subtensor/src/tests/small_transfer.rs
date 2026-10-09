#![allow(clippy::unwrap_used)]

use super::mock::*;
use crate::*;
use frame_support::{assert_noop, assert_ok};
use pallet_subtensor_proxy as pallet_proxy;
use sp_core::U256;
use subtensor_runtime_common::{ProxyType, SMALL_TRANSFER_LIMIT, TaoBalance};

const TAO: u64 = 1_000_000_000;

fn fund(account: U256, tao: u64) {
    add_balance_to_coldkey_account(&account, TaoBalance::from(tao));
}

fn small_transfer(origin: U256, destination: U256, amount: u64) -> DispatchResult {
    SubtensorModule::small_transfer(
        RuntimeOrigin::signed(origin),
        destination,
        TaoBalance::from(amount),
    )
}

fn set_destination(coldkey: U256, destination: Option<U256>) {
    assert_ok!(SubtensorModule::set_small_transfer_destination(
        RuntimeOrigin::signed(coldkey),
        destination
    ));
}

#[test]
fn test_set_small_transfer_destination_sets_clears_and_emits() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let destination = U256::from(2);

        assert_eq!(SmallTransferDestination::<Test>::get(coldkey), None);

        set_destination(coldkey, Some(destination));
        assert_eq!(
            SmallTransferDestination::<Test>::get(coldkey),
            Some(destination)
        );
        System::assert_last_event(
            Event::SmallTransferDestinationSet {
                coldkey,
                destination: Some(destination),
            }
            .into(),
        );

        set_destination(coldkey, None);
        assert_eq!(SmallTransferDestination::<Test>::get(coldkey), None);
        System::assert_last_event(
            Event::SmallTransferDestinationSet {
                coldkey,
                destination: None,
            }
            .into(),
        );
    });
}

#[test]
fn test_small_transfer_requires_a_whitelisted_destination() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let destination = U256::from(2);
        fund(coldkey, 10 * TAO);

        // Nothing whitelisted: refused.
        assert_noop!(
            small_transfer(coldkey, destination, 1_000),
            Error::<Test>::SmallTransferDestinationNotAllowed
        );

        // Another account whitelisted: still refused.
        set_destination(coldkey, Some(U256::from(3)));
        assert_noop!(
            small_transfer(coldkey, destination, 1_000),
            Error::<Test>::SmallTransferDestinationNotAllowed
        );

        // The whitelisted destination: accepted.
        set_destination(coldkey, Some(destination));
        assert_ok!(small_transfer(coldkey, destination, 1_000));
    });
}

#[test]
fn test_small_transfer_amount_is_strictly_below_limit() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let destination = U256::from(2);
        fund(coldkey, 10 * TAO);
        set_destination(coldkey, Some(destination));

        assert_noop!(
            small_transfer(coldkey, destination, SMALL_TRANSFER_LIMIT.to_u64()),
            Error::<Test>::SmallTransferAmountTooHigh
        );
        assert_noop!(
            small_transfer(coldkey, destination, SMALL_TRANSFER_LIMIT.to_u64() + 1),
            Error::<Test>::SmallTransferAmountTooHigh
        );
        assert_ok!(small_transfer(
            coldkey,
            destination,
            SMALL_TRANSFER_LIMIT.to_u64() - 1
        ));
    });
}

#[test]
fn test_small_transfer_moves_balance_and_records_block() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let destination = U256::from(2);
        fund(coldkey, 10 * TAO);
        fund(destination, TAO);
        set_destination(coldkey, Some(destination));
        System::set_block_number(42);

        assert_ok!(small_transfer(coldkey, destination, 1_000));

        assert_eq!(
            Balances::free_balance(coldkey),
            TaoBalance::from(10 * TAO - 1_000)
        );
        assert_eq!(
            Balances::free_balance(destination),
            TaoBalance::from(TAO + 1_000)
        );
        assert_eq!(
            LastRateLimitedBlock::<Test>::get(RateLimitKey::SmallTransfer(coldkey)),
            42
        );
    });
}

#[test]
fn test_small_transfer_at_most_once_per_block() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let destination = U256::from(2);
        fund(coldkey, 10 * TAO);
        set_destination(coldkey, Some(destination));

        assert_ok!(small_transfer(coldkey, destination, 1_000));
        assert_noop!(
            small_transfer(coldkey, destination, 1_000),
            Error::<Test>::SmallTransferRateLimitExceeded
        );

        // The very next block is enough.
        System::set_block_number(2);
        assert_ok!(small_transfer(coldkey, destination, 1_000));
        assert_noop!(
            small_transfer(coldkey, destination, 1_000),
            Error::<Test>::SmallTransferRateLimitExceeded
        );
    });
}

#[test]
fn test_small_transfer_rate_limit_is_per_coldkey() {
    new_test_ext(1).execute_with(|| {
        let alice = U256::from(1);
        let bob = U256::from(2);
        let destination = U256::from(3);
        fund(alice, 10 * TAO);
        fund(bob, 10 * TAO);
        set_destination(alice, Some(destination));
        set_destination(bob, Some(destination));

        assert_ok!(small_transfer(alice, destination, 1_000));
        // Bob's own limit is untouched by Alice's transfer.
        assert_ok!(small_transfer(bob, destination, 1_000));
    });
}

#[test]
fn test_small_transfer_clearing_destination_blocks_transfers() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let destination = U256::from(2);
        fund(coldkey, 10 * TAO);
        set_destination(coldkey, Some(destination));
        assert_ok!(small_transfer(coldkey, destination, 1_000));

        System::set_block_number(2);
        set_destination(coldkey, None);
        assert_noop!(
            small_transfer(coldkey, destination, 1_000),
            Error::<Test>::SmallTransferDestinationNotAllowed
        );
    });
}

#[test]
fn test_small_transfer_keeps_payer_alive() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let destination = U256::from(2);
        let balance = 1_000;
        fund(coldkey, balance);
        set_destination(coldkey, Some(destination));

        // Sending the whole balance would reap the payer: refused, nothing moves.
        assert!(small_transfer(coldkey, destination, balance).is_err());
        assert_eq!(Balances::free_balance(coldkey), TaoBalance::from(balance));
        assert_eq!(
            LastRateLimitedBlock::<Test>::get(RateLimitKey::SmallTransfer(coldkey)),
            0
        );
    });
}

/// Through the proxy pallet the call runs as the delegating coldkey, so the
/// whitelist and the per-block limit bind every delegate of that coldkey
/// together, not each delegate separately.
#[test]
fn test_small_transfer_through_proxies_shares_the_coldkey_limit() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let delegate_a = U256::from(2);
        let delegate_b = U256::from(3);
        let destination = U256::from(4);
        let stranger = U256::from(5);
        fund(coldkey, 10 * TAO);
        set_destination(coldkey, Some(destination));
        for delegate in [delegate_a, delegate_b] {
            assert_ok!(Proxy::add_proxy(
                RuntimeOrigin::signed(coldkey),
                delegate,
                ProxyType::SmallTransfer,
                0
            ));
        }

        let proxied = |delegate: U256, destination: U256| {
            Proxy::proxy(
                RuntimeOrigin::signed(delegate),
                coldkey,
                Some(ProxyType::SmallTransfer),
                Box::new(RuntimeCall::SubtensorModule(crate::Call::small_transfer {
                    destination,
                    amount: TaoBalance::from(1_000),
                })),
            )
        };
        let last_result = || pallet_proxy::LastCallResult::<Test>::get(coldkey).unwrap();

        // A delegate cannot pay anyone but the whitelisted destination.
        assert_ok!(proxied(delegate_a, stranger));
        assert_eq!(
            last_result(),
            Err(Error::<Test>::SmallTransferDestinationNotAllowed.into())
        );

        // First delegate pays; the second one is refused in the same block.
        assert_ok!(proxied(delegate_a, destination));
        assert_eq!(last_result(), Ok(()));
        assert_ok!(proxied(delegate_b, destination));
        assert_eq!(
            last_result(),
            Err(Error::<Test>::SmallTransferRateLimitExceeded.into())
        );

        // Next block, either delegate may pay again.
        System::set_block_number(2);
        assert_ok!(proxied(delegate_b, destination));
        assert_eq!(last_result(), Ok(()));
        assert_eq!(
            Balances::free_balance(destination),
            TaoBalance::from(2 * 1_000)
        );
    });
}
