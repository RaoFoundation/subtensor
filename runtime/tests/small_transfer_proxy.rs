//! End-to-end checks of the `SmallTransfer` proxy on the real runtime: the real proxy
//! pallet dispatches through the real filter into the real `small_transfer` call, so the
//! allowlist, the whitelisted destination and the once-per-block limit are exercised
//! together, as a delegate would hit them on chain.
#![allow(clippy::unwrap_used)]

use frame_support::assert_ok;
use node_subtensor_runtime::{
    BuildStorage, Runtime, RuntimeCall, RuntimeGenesisConfig, RuntimeOrigin, System,
};
use sp_runtime::{
    DispatchError,
    traits::{Dispatchable, StaticLookup},
};
use subtensor_runtime_common::{AccountId, ProxyType, SMALL_TRANSFER_LIMIT, TaoBalance, Token};

type Proxy = pallet_subtensor_proxy::Pallet<Runtime>;
type Subtensor = pallet_subtensor::Pallet<Runtime>;
type Balances = pallet_balances::Pallet<Runtime>;
type Lookup = <<Runtime as frame_system::Config>::Lookup as StaticLookup>::Source;

const TAO: u64 = 1_000_000_000;

fn new_test_ext() -> sp_io::TestExternalities {
    let mut ext: sp_io::TestExternalities = RuntimeGenesisConfig::default()
        .build_storage()
        .unwrap()
        .into();
    ext.execute_with(|| System::set_block_number(1));
    ext
}

fn fund(account: &AccountId, tao: u64) {
    let credit = Subtensor::mint_tao(TaoBalance::from(tao));
    let _ = Subtensor::spend_tao(account, credit, TaoBalance::from(tao)).unwrap();
}

fn free(account: &AccountId) -> u64 {
    Balances::free_balance(account).to_u64()
}

/// Dispatches `inner` through `Proxy.proxy` signed by `delegate` and returns what the
/// inner call did (the outer proxy call succeeds even when the inner one is refused).
fn proxied(
    delegate: &AccountId,
    real: &AccountId,
    proxy_type: ProxyType,
    inner: RuntimeCall,
) -> Result<(), DispatchError> {
    let call = RuntimeCall::Proxy(pallet_subtensor_proxy::Call::proxy {
        real: Lookup::from(real.clone()),
        force_proxy_type: Some(proxy_type),
        call: Box::new(inner),
    });
    assert_ok!(call.dispatch(RuntimeOrigin::signed(delegate.clone())));
    pallet_subtensor_proxy::LastCallResult::<Runtime>::get(real).unwrap()
}

fn small_transfer(destination: &AccountId, amount: u64) -> RuntimeCall {
    RuntimeCall::SubtensorModule(pallet_subtensor::Call::small_transfer {
        destination: destination.clone(),
        amount: TaoBalance::from(amount),
    })
}

fn transfer_keep_alive(destination: &AccountId, amount: u64) -> RuntimeCall {
    RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
        dest: Lookup::from(destination.clone()),
        value: TaoBalance::from(amount),
    })
}

fn set_destination(destination: Option<&AccountId>) -> RuntimeCall {
    RuntimeCall::SubtensorModule(pallet_subtensor::Call::set_small_transfer_destination {
        destination: destination.cloned(),
    })
}

struct Setup {
    user: AccountId,
    /// The user's free balance once the proxy deposits are reserved.
    user_free: u64,
    delegate_a: AccountId,
    delegate_b: AccountId,
    fee_wallet: AccountId,
    stranger: AccountId,
}

/// A user coldkey with two `SmallTransfer` delegates, no destination set yet.
fn setup() -> Setup {
    let mut s = Setup {
        user: AccountId::from([1u8; 32]),
        user_free: 0,
        delegate_a: AccountId::from([2u8; 32]),
        delegate_b: AccountId::from([3u8; 32]),
        fee_wallet: AccountId::from([4u8; 32]),
        stranger: AccountId::from([5u8; 32]),
    };
    fund(&s.user, 10 * TAO);
    fund(&s.delegate_a, TAO);
    fund(&s.delegate_b, TAO);
    fund(&s.fee_wallet, TAO);
    for delegate in [&s.delegate_a, &s.delegate_b] {
        Proxy::add_proxy_delegate(&s.user, delegate.clone(), ProxyType::SmallTransfer, 0).unwrap();
    }
    s.user_free = free(&s.user);
    s
}

#[test]
fn small_transfer_proxy_only_dispatches_small_transfer() {
    new_test_ext().execute_with(|| {
        let s = setup();
        let filtered: DispatchError = frame_system::Error::<Runtime>::CallFiltered.into();

        // The raw balance transfer that the proxy type used to admit is now filtered out,
        // whatever the amount.
        assert_eq!(
            proxied(
                &s.delegate_a,
                &s.user,
                ProxyType::SmallTransfer,
                transfer_keep_alive(&s.fee_wallet, 1_000),
            ),
            Err(filtered)
        );

        // The delegate cannot grant itself a destination: the whitelist call is owner-only.
        assert_eq!(
            proxied(
                &s.delegate_a,
                &s.user,
                ProxyType::SmallTransfer,
                set_destination(Some(&s.delegate_a)),
            ),
            Err(filtered)
        );

        // At or above the limit the filter refuses before the call even runs.
        assert_eq!(
            proxied(
                &s.delegate_a,
                &s.user,
                ProxyType::SmallTransfer,
                small_transfer(&s.fee_wallet, SMALL_TRANSFER_LIMIT.to_u64()),
            ),
            Err(filtered)
        );
        assert_eq!(free(&s.user), s.user_free);
    });
}

#[test]
fn small_transfer_proxy_pays_only_the_whitelisted_destination() {
    new_test_ext().execute_with(|| {
        let s = setup();
        let not_allowed: DispatchError =
            pallet_subtensor::Error::<Runtime>::SmallTransferDestinationNotAllowed.into();

        // No destination whitelisted: the proxy can move nothing.
        assert_eq!(
            proxied(
                &s.delegate_a,
                &s.user,
                ProxyType::SmallTransfer,
                small_transfer(&s.fee_wallet, 1_000),
            ),
            Err(not_allowed)
        );

        // The user whitelists the fee wallet with a plain signed call.
        assert_ok!(
            set_destination(Some(&s.fee_wallet)).dispatch(RuntimeOrigin::signed(s.user.clone()))
        );

        // Anyone else is still refused; the fee wallet is paid.
        assert_eq!(
            proxied(
                &s.delegate_a,
                &s.user,
                ProxyType::SmallTransfer,
                small_transfer(&s.stranger, 1_000),
            ),
            Err(not_allowed)
        );
        assert_eq!(
            proxied(
                &s.delegate_a,
                &s.user,
                ProxyType::SmallTransfer,
                small_transfer(&s.fee_wallet, 1_000),
            ),
            Ok(())
        );
        assert_eq!(free(&s.user), s.user_free - 1_000);
        assert_eq!(free(&s.fee_wallet), TAO + 1_000);
        assert_eq!(free(&s.stranger), 0);
    });
}

#[test]
fn small_transfer_proxy_is_limited_to_one_call_per_block_per_coldkey() {
    new_test_ext().execute_with(|| {
        let s = setup();
        let too_fast: DispatchError =
            pallet_subtensor::Error::<Runtime>::SmallTransferRateLimitExceeded.into();
        assert_ok!(
            set_destination(Some(&s.fee_wallet)).dispatch(RuntimeOrigin::signed(s.user.clone()))
        );

        // First delegate pays; the second delegate of the same coldkey is refused in the
        // same block: the limit is on the coldkey, not on the delegate.
        assert_eq!(
            proxied(
                &s.delegate_a,
                &s.user,
                ProxyType::SmallTransfer,
                small_transfer(&s.fee_wallet, 1_000),
            ),
            Ok(())
        );
        assert_eq!(
            proxied(
                &s.delegate_b,
                &s.user,
                ProxyType::SmallTransfer,
                small_transfer(&s.fee_wallet, 1_000),
            ),
            Err(too_fast)
        );
        assert_eq!(
            proxied(
                &s.delegate_a,
                &s.user,
                ProxyType::SmallTransfer,
                small_transfer(&s.fee_wallet, 1_000),
            ),
            Err(too_fast)
        );

        // The next block frees the slot.
        System::set_block_number(2);
        assert_eq!(
            proxied(
                &s.delegate_b,
                &s.user,
                ProxyType::SmallTransfer,
                small_transfer(&s.fee_wallet, 1_000),
            ),
            Ok(())
        );
        assert_eq!(free(&s.fee_wallet), TAO + 2_000);
        assert_eq!(free(&s.user), s.user_free - 2_000);
    });
}

#[test]
fn transfer_proxy_still_grants_small_transfer_but_the_call_keeps_its_own_cap() {
    new_test_ext().execute_with(|| {
        let s = setup();
        let transfer_delegate = AccountId::from([6u8; 32]);
        fund(&transfer_delegate, TAO);
        Proxy::add_proxy_delegate(&s.user, transfer_delegate.clone(), ProxyType::Transfer, 0)
            .unwrap();
        assert_ok!(
            set_destination(Some(&s.fee_wallet)).dispatch(RuntimeOrigin::signed(s.user.clone()))
        );

        // `Transfer` is unconditional in the filter, so the at-limit amount reaches the
        // call, which enforces the cap itself.
        assert_eq!(
            proxied(
                &transfer_delegate,
                &s.user,
                ProxyType::Transfer,
                small_transfer(&s.fee_wallet, SMALL_TRANSFER_LIMIT.to_u64()),
            ),
            Err(pallet_subtensor::Error::<Runtime>::SmallTransferAmountTooHigh.into())
        );
        assert_eq!(
            proxied(
                &transfer_delegate,
                &s.user,
                ProxyType::Transfer,
                small_transfer(&s.fee_wallet, SMALL_TRANSFER_LIMIT.to_u64() - 1),
            ),
            Ok(())
        );
    });
}
