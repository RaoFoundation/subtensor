#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]

use frame_support::{assert_noop, assert_ok};
use sp_core::H160;
use sp_runtime::DispatchError;
use subtensor_runtime_common::{NetUid, TaoBalance, TusdBalance, rails::OriginChain};

use crate::{
    AlphaSupply, Deferred, Destination, Error, Event, Gateway, Markets, Message, MessageCeiling,
    MessageReserve, NativeAsset, NextMessage, NextOrder, NextOutbound, Order, Outbound, Outbox,
    Outcome, Paused, RailsStaking, Receipts, SpokeAsset, SubnetStatus, TaoCap, TaoSupply,
    TokenInfo, Tokens,
    mock::*,
    router::{RATE_SCALE, VIRTUAL, alpha_for_shares, rate, shares_for_alpha},
};

const ALICE: AccountId = account(1);
const BOB: AccountId = account(2);
const SERVICE: AccountId = account(77);
/// An account that takes no credits.
const STRANGER: AccountId = account(78);
const CHUTES: u16 = 64;
const LIUM: u16 = 51;

fn payer() -> H160 {
    H160::repeat_byte(0xAA)
}

fn wallet() -> H160 {
    H160::repeat_byte(0xBB)
}

fn netuid(raw: u16) -> NetUid {
    NetUid::from(raw)
}

fn order(source: SpokeAsset, amount: u64, destination: Destination<AccountId>) -> Order<AccountId> {
    Order {
        nonce: NextOrder::<Test>::get(BASE),
        payer: payer(),
        source,
        amount,
        destination,
        min_out: 0,
        deadline: u64::MAX,
    }
}

fn execute(order: Order<AccountId>) -> sp_runtime::DispatchResult {
    Rails::execute(RuntimeOrigin::signed(GATEWAY), BASE, portal(BASE), order)
}

/// Settles an order from Base and returns what became of it.
fn settle(source: SpokeAsset, amount: u64, destination: Destination<AccountId>) -> Outcome {
    let order = order(source, amount, destination);
    let nonce = order.nonce;
    assert_ok!(execute(order));
    assert_hub_empty();
    Receipts::<Test>::get(BASE, nonce).unwrap().outcome
}

/// Settles an order that must succeed and returns what it delivered.
fn settled(source: SpokeAsset, amount: u64, destination: Destination<AccountId>) -> u64 {
    match settle(source, amount, destination) {
        Outcome::Settled { amount_out } => amount_out,
        other => panic!("order was not settled: {other:?}"),
    }
}

/// Settles an order that must be refunded and returns why.
fn refunded(source: SpokeAsset, amount: u64, destination: Destination<AccountId>) -> DispatchError {
    let queued = outbox().len();
    let nonce = NextOrder::<Test>::get(BASE);
    let outcome = settle(source, amount, destination);
    let Outcome::Refunded { error } = outcome else {
        panic!("order was not refunded: {outcome:?}");
    };
    // The spoke is told to give back exactly what it took.
    let messages = outbox();
    assert_eq!(messages.len(), queued + 1);
    assert_eq!(
        messages.last().unwrap().message,
        Message::Credit {
            asset: source,
            to: payer(),
            amount,
            order: Some(nonce),
            refund: true,
            rate: 0,
        }
    );
    error
}

/// The bridges of the mock whose spokes receive messages.
const SPOKES: [u32; 2] = [BASE, ETHEREUM];

/// Every message that waits for the gateway, bridge by bridge, each queue in order.
fn outbox() -> Vec<Outbound> {
    SPOKES
        .into_iter()
        .flat_map(|bridge| {
            (NextOutbound::<Test>::get(bridge)..NextMessage::<Test>::get(bridge)).map(
                move |nonce| Outbound {
                    bridge,
                    nonce,
                    message: Outbox::<Test>::get(bridge, nonce).unwrap(),
                },
            )
        })
        .collect()
}

/// The gateway hands every waiting message to a transport that charges nothing.
fn flush() -> Vec<Outbound> {
    let mut sent = Vec::new();
    for bridge in SPOKES {
        while let Some((domain, sender, outbound)) = Rails::next_outbound(bridge) {
            assert_eq!((domain, sender), (bridge, portal(bridge)));
            assert_eq!(outbound.bridge, bridge);
            assert_ok!(Rails::ack_outbound(
                RuntimeOrigin::signed(GATEWAY),
                bridge,
                outbound.nonce,
                0.into()
            ));
            sent.push(outbound);
        }
    }
    sent
}

/// Puts the existential deposit in the vault account of a subnet, as its first stake or
/// spend would leave it there, so that a test can count in whole amounts.
fn keep_vault_alive(raw: u16) {
    fund_tao(Rails::vault_account(netuid(raw)), EXISTENTIAL_DEPOSIT);
}

/// Opens a market for a new subnet and lists its token on Base.
fn open_market(raw: u16, name: &str) {
    add_subnet(raw, name);
    keep_vault_alive(raw);
    assert_ok!(Rails::open_market(
        RuntimeOrigin::root(),
        netuid(raw),
        (1_000_000 * UNIT).into()
    ));
    assert_ok!(Rails::list_market(
        RuntimeOrigin::root(),
        netuid(raw),
        BASE,
        H160::repeat_byte(raw as u8)
    ));
    flush();
}

fn hub_tao() -> u64 {
    Balances::free_balance(Rails::hub_account()).into()
}

fn assert_hub_empty() {
    assert_eq!(hub_tao(), 0, "TAO was left in the hub account");
    assert_eq!(
        Tusd::balance(&Rails::hub_account()),
        TusdBalance::from(0),
        "tUSD was left in the hub account"
    );
}

fn outstanding(bridge: u32) -> u64 {
    pallet_tusd::BridgeOutstanding::<Test>::get(bridge).into()
}

fn assert_invariants() {
    assert_hub_empty();
    Tusd::do_try_state().unwrap();
    Pool::do_try_state().unwrap();
    Rails::do_try_state().unwrap();
    // A live market's vault covers its shares.
    for (market, terms) in Markets::<Test>::iter() {
        let staked: u64 = MockStaking::staked(&Rails::vault_account(market), market).into();
        if matches!(
            MockStaking::subnet_status(market),
            SubnetStatus::Live { .. }
        ) {
            assert!(alpha_for_shares(terms.shares, staked, terms.shares).unwrap() <= staked);
        }
    }
}

// ---------------------------------------------------------------- who may call

#[test]
fn only_the_gateway_delivers_orders() {
    new_test_ext().execute_with(|| {
        let order = order(
            SpokeAsset::Stable,
            UNIT,
            Destination::NativeTusd { to: ALICE },
        );
        for origin in [
            RuntimeOrigin::signed(ALICE),
            RuntimeOrigin::signed(GUARDIAN),
            RuntimeOrigin::root(),
            RuntimeOrigin::none(),
        ] {
            assert!(Rails::execute(origin, BASE, portal(BASE), order.clone()).is_err());
        }
        assert_noop!(
            Rails::execute(
                RuntimeOrigin::signed(ALICE),
                BASE,
                portal(BASE),
                order.clone()
            ),
            Error::<Test>::NotGateway
        );
        // Without a gateway nobody does.
        assert_ok!(Rails::set_gateway(RuntimeOrigin::root(), None));
        assert_noop!(execute(order), Error::<Test>::NotGateway);
    });
}

#[test]
fn orders_come_only_from_the_portal_of_a_live_bridge() {
    new_test_ext().execute_with(|| {
        let order = order(
            SpokeAsset::Stable,
            UNIT,
            Destination::NativeTusd { to: ALICE },
        );
        // Base's portal address on another domain, and another address on Base's domain.
        for (domain, sender) in [
            (ETHEREUM, portal(BASE)),
            (BASE, portal(ETHEREUM)),
            (7, portal(7)),
        ] {
            assert_noop!(
                Rails::execute(
                    RuntimeOrigin::signed(GATEWAY),
                    domain,
                    sender,
                    order.clone()
                ),
                Error::<Test>::UnknownPortal
            );
        }
        assert_ok!(Tusd::set_bridge_enabled(
            RuntimeOrigin::signed(GUARDIAN),
            BASE,
            false
        ));
        assert_noop!(execute(order.clone()), Error::<Test>::BridgeDisabled);
        assert_ok!(Tusd::set_bridge_enabled(RuntimeOrigin::root(), BASE, true));
        assert_ok!(execute(order));
    });
}

#[test]
fn each_order_settles_once_and_in_sequence() {
    new_test_ext().execute_with(|| {
        let first = order(
            SpokeAsset::Stable,
            UNIT,
            Destination::NativeTusd { to: ALICE },
        );
        let mut second = first.clone();
        second.nonce = 1;
        // The second cannot go before the first.
        assert_noop!(execute(second.clone()), Error::<Test>::OrderOutOfSequence);
        assert_ok!(execute(first.clone()));
        // The first cannot go twice.
        assert_noop!(execute(first), Error::<Test>::OrderOutOfSequence);
        assert_ok!(execute(second.clone()));
        assert_noop!(execute(second), Error::<Test>::OrderOutOfSequence);
        assert_eq!(Tusd::balance(&ALICE), TusdBalance::from(2 * UNIT));
        assert_eq!(NextOrder::<Test>::get(BASE), 2);
        // Bridges count on their own.
        assert_eq!(NextOrder::<Test>::get(ETHEREUM), 0);
        let from_ethereum = Order {
            nonce: 0,
            ..order(
                SpokeAsset::Stable,
                UNIT,
                Destination::NativeTusd { to: BOB },
            )
        };
        assert_ok!(Rails::execute(
            RuntimeOrigin::signed(GATEWAY),
            ETHEREUM,
            portal(ETHEREUM),
            from_ethereum
        ));
        assert_eq!(outstanding(ETHEREUM), UNIT);
        assert_invariants();
    });
}

#[test]
fn guardian_pauses_and_only_admin_resumes() {
    new_test_ext().execute_with(|| {
        assert_noop!(
            Rails::set_paused(RuntimeOrigin::signed(ALICE), true),
            DispatchError::BadOrigin
        );
        assert_ok!(Rails::set_paused(RuntimeOrigin::signed(GUARDIAN), true));
        assert!(Paused::<Test>::get());

        // A paused hub takes nothing and hands nothing over. The order stays with the
        // transport, which delivers it again later.
        let order = order(
            SpokeAsset::Stable,
            UNIT,
            Destination::NativeTusd { to: ALICE },
        );
        assert_noop!(execute(order.clone()), Error::<Test>::Paused);
        fund_tusd(ALICE, 10 * UNIT);
        assert_noop!(
            Rails::bridge_out(
                RuntimeOrigin::signed(ALICE),
                BASE,
                NativeAsset::Tusd,
                UNIT,
                Destination::Stable { to: wallet() },
                0
            ),
            Error::<Test>::Paused
        );

        assert_noop!(
            Rails::set_paused(RuntimeOrigin::signed(GUARDIAN), false),
            DispatchError::BadOrigin
        );
        assert_ok!(Rails::set_paused(RuntimeOrigin::root(), false));
        assert_ok!(execute(order));
    });
}

#[test]
fn guardian_also_pauses_the_pool_and_disables_bridges() {
    new_test_ext().execute_with(|| {
        assert_ok!(Pool::set_enabled(RuntimeOrigin::signed(GUARDIAN), false));
        assert_noop!(
            Pool::set_enabled(RuntimeOrigin::signed(GUARDIAN), true),
            DispatchError::BadOrigin
        );
        assert_ok!(Tusd::set_bridge_enabled(
            RuntimeOrigin::signed(GUARDIAN),
            BASE,
            false
        ));
        // A removed guardian has no power left.
        assert_ok!(Rails::set_guardian(RuntimeOrigin::root(), None));
        assert_noop!(
            Tusd::set_bridge_enabled(RuntimeOrigin::signed(GUARDIAN), ETHEREUM, false),
            DispatchError::BadOrigin
        );
    });
}

#[test]
fn settings_are_admin_only() {
    new_test_ext().execute_with(|| {
        add_subnet(CHUTES, "Chutes");
        let signed = || RuntimeOrigin::signed(GUARDIAN);
        assert_noop!(
            Rails::set_gateway(signed(), Some(ALICE)),
            DispatchError::BadOrigin
        );
        assert_noop!(
            Rails::set_guardian(signed(), Some(ALICE)),
            DispatchError::BadOrigin
        );
        assert_noop!(
            Rails::register_token(signed(), BASE, SpokeAsset::Tao, wallet()),
            DispatchError::BadOrigin
        );
        assert_noop!(
            Rails::set_tao_cap(signed(), BASE, TaoBalance::from(1)),
            DispatchError::BadOrigin
        );
        assert_noop!(
            Rails::open_market(signed(), netuid(CHUTES), UNIT.into()),
            DispatchError::BadOrigin
        );
        assert_noop!(
            Rails::list_market(signed(), netuid(CHUTES), BASE, wallet()),
            DispatchError::BadOrigin
        );
        assert_noop!(
            Rails::set_market_cap(signed(), netuid(CHUTES), UNIT.into()),
            DispatchError::BadOrigin
        );
        assert_noop!(
            Rails::skip_order(signed(), BASE, 0),
            DispatchError::BadOrigin
        );
        assert_noop!(
            Rails::set_message_reserve(signed(), BASE, TaoBalance::from(1), TaoBalance::from(1)),
            DispatchError::BadOrigin
        );
        assert_noop!(
            Rails::ack_outbound(signed(), BASE, 0, 0.into()),
            Error::<Test>::NotGateway
        );
        assert_noop!(
            Rails::defer_outbound(signed(), BASE, 0),
            Error::<Test>::NotGateway
        );
    });
}

// ---------------------------------------------------------------- stablecoin in

#[test]
fn stablecoin_arrives_as_tusd_at_an_account() {
    new_test_ext().execute_with(|| {
        let delivered = settled(
            SpokeAsset::Stable,
            250 * UNIT,
            Destination::NativeTusd { to: ALICE },
        );

        assert_eq!(delivered, 250 * UNIT);
        assert_eq!(Tusd::balance(&ALICE), TusdBalance::from(250 * UNIT));
        assert_eq!(outstanding(BASE), 250 * UNIT);
        assert!(outbox().is_empty(), "nothing is paid out on the spoke");
        System::assert_has_event(
            Event::OrderSettled {
                bridge: BASE,
                nonce: 0,
                payer: payer(),
                source: SpokeAsset::Stable,
                amount: 250 * UNIT,
                destination: Destination::NativeTusd { to: ALICE },
                amount_out: 250 * UNIT,
            }
            .into(),
        );
        assert_invariants();
    });
}

#[test]
fn stablecoin_arrives_as_tao_at_an_account() {
    new_test_ext().execute_with(|| {
        let quoted: u64 = Pool::quote_tusd_for_tao((4_000 * UNIT).into())
            .unwrap()
            .into();

        let delivered = settled(
            SpokeAsset::Stable,
            4_000 * UNIT,
            Destination::NativeTao { to: ALICE },
        );

        assert_eq!(delivered, quoted);
        assert!(delivered > 9 * UNIT && delivered < 10 * UNIT);
        assert_eq!(Balances::free_balance(ALICE), TaoBalance::from(delivered));
        // The dollars are in the pool now, still outstanding at the bridge.
        assert_eq!(outstanding(BASE), 4_000 * UNIT);
        assert_eq!(Pool::reserves().tusd, 4_004_000 * UNIT);
        assert_invariants();
    });
}

#[test]
fn stablecoin_buys_canonical_tao_backed_by_the_reserve() {
    new_test_ext().execute_with(|| {
        let delivered = settled(
            SpokeAsset::Stable,
            4_000 * UNIT,
            Destination::Tao { to: wallet() },
        );

        assert_eq!(
            Balances::free_balance(Rails::reserve_account()),
            TaoBalance::from(delivered)
        );
        assert_eq!(TaoSupply::<Test>::get(BASE), TaoBalance::from(delivered));
        assert_eq!(
            outbox(),
            vec![Outbound {
                bridge: BASE,
                nonce: 0,
                message: Message::Credit {
                    asset: SpokeAsset::Tao,
                    to: wallet(),
                    amount: delivered,
                    order: Some(0),
                    refund: false,
                    rate: 0,
                },
            }]
        );
        assert_invariants();
    });
}

#[test]
fn canonical_tao_is_capped_per_spoke_and_needs_a_registered_token() {
    new_test_ext().execute_with(|| {
        assert_ok!(Rails::set_tao_cap(
            RuntimeOrigin::root(),
            BASE,
            (5 * UNIT).into()
        ));
        assert_eq!(
            refunded(
                SpokeAsset::Stable,
                4_000 * UNIT,
                Destination::Tao { to: wallet() }
            ),
            Error::<Test>::TaoCapExceeded.into()
        );
        assert_eq!(TaoCap::<Test>::get(BASE), TaoBalance::from(5 * UNIT));
        settled(
            SpokeAsset::Stable,
            1_000 * UNIT,
            Destination::Tao { to: wallet() },
        );

        // Ethereum has no canonical TAO token yet.
        let mut order = order(
            SpokeAsset::Stable,
            1_000 * UNIT,
            Destination::Tao { to: wallet() },
        );
        order.nonce = 0;
        assert_ok!(Rails::execute(
            RuntimeOrigin::signed(GATEWAY),
            ETHEREUM,
            portal(ETHEREUM),
            order
        ));
        assert_eq!(
            Receipts::<Test>::get(ETHEREUM, 0).unwrap().outcome,
            Outcome::Refunded {
                error: Error::<Test>::TokenNotRegistered.into()
            }
        );
        assert_eq!(outstanding(ETHEREUM), 0);
        assert_invariants();
    });
}

// ---------------------------------------------------------------- canonical TAO back

#[test]
fn canonical_tao_sells_for_the_stablecoin() {
    new_test_ext().execute_with(|| {
        let bought = settled(
            SpokeAsset::Stable,
            4_000 * UNIT,
            Destination::Tao { to: wallet() },
        );
        flush();
        let quoted: u64 = Pool::quote_tao_for_tusd(bought.into()).unwrap().into();

        // The holder burns the token on Base and asks for dollars.
        let paid = settled(
            SpokeAsset::Tao,
            bought,
            Destination::Stable { to: wallet() },
        );

        // She is paid whole units of the stablecoin: one is a thousand of the hub's units.
        assert_eq!(paid, quoted - quoted % 1_000);
        // Two fees of 0.3% and the price moved by her own trades.
        assert!(paid > 3_970 * UNIT && paid < 3_980 * UNIT);
        assert_eq!(TaoSupply::<Test>::get(BASE), TaoBalance::from(0));
        assert_eq!(
            Balances::free_balance(Rails::reserve_account()),
            TaoBalance::from(0)
        );
        // The bridge burned all the tUSD of the sale, the part below one unit too: the
        // rest of her dollars stayed in the pool.
        assert_eq!(outstanding(BASE), 4_000 * UNIT - quoted);
        assert_eq!(
            flush()[0].message,
            Message::Credit {
                asset: SpokeAsset::Stable,
                to: wallet(),
                amount: paid,
                order: Some(1),
                refund: false,
                rate: 0,
            }
        );
        assert_invariants();
    });
}

#[test]
fn canonical_tao_unwraps_to_an_account() {
    new_test_ext().execute_with(|| {
        let bought = settled(
            SpokeAsset::Stable,
            4_000 * UNIT,
            Destination::Tao { to: wallet() },
        );
        let unwrapped = settled(
            SpokeAsset::Tao,
            bought,
            Destination::NativeTao { to: ALICE },
        );
        assert_eq!(unwrapped, bought);
        assert_eq!(Balances::free_balance(ALICE), TaoBalance::from(bought));
        assert_eq!(TaoSupply::<Test>::get(BASE), TaoBalance::from(0));
        assert_invariants();
    });
}

#[test]
fn a_spoke_cannot_redeem_tokens_the_hub_never_let_it_mint() {
    new_test_ext().execute_with(|| {
        open_market(CHUTES, "Chutes");
        let bought = settled(
            SpokeAsset::Stable,
            4_000 * UNIT,
            Destination::Tao { to: wallet() },
        );
        settled(
            SpokeAsset::Stable,
            4_000 * UNIT,
            Destination::Alpha {
                netuid: netuid(CHUTES),
                to: wallet(),
            },
        );
        let shares = AlphaSupply::<Test>::get(netuid(CHUTES), BASE);
        flush();
        let reserve = Balances::free_balance(Rails::reserve_account());

        // A compromised portal claims burns beyond its supply. Nothing is released and
        // nothing is refunded: there is nothing of the hub's to give back.
        for (source, amount) in [
            (SpokeAsset::Tao, bought + 1),
            (SpokeAsset::Alpha(netuid(CHUTES)), shares + 1),
            (SpokeAsset::Alpha(netuid(LIUM)), 1),
        ] {
            let outcome = settle(source, amount, Destination::Stable { to: wallet() });
            assert_eq!(
                outcome,
                Outcome::Rejected {
                    error: Error::<Test>::Unbacked.into()
                }
            );
        }
        assert!(outbox().is_empty());
        assert_eq!(Balances::free_balance(Rails::reserve_account()), reserve);
        assert_eq!(AlphaSupply::<Test>::get(netuid(CHUTES), BASE), shares);
        // Ethereum minted none, so it redeems none of Base's.
        let from_ethereum = Order {
            nonce: 0,
            ..order(SpokeAsset::Tao, 1, Destination::Stable { to: wallet() })
        };
        assert_ok!(Rails::execute(
            RuntimeOrigin::signed(GATEWAY),
            ETHEREUM,
            portal(ETHEREUM),
            from_ethereum
        ));
        assert!(matches!(
            Receipts::<Test>::get(ETHEREUM, 0).unwrap().outcome,
            Outcome::Rejected { .. }
        ));
        assert_invariants();
    });
}

#[test]
fn the_reserve_is_never_left_with_dust() {
    new_test_ext().execute_with(|| {
        let bought = settled(
            SpokeAsset::Stable,
            4_000 * UNIT,
            Destination::Tao { to: wallet() },
        );
        flush();
        // Leaving less than an account can hold would strand it.
        assert_eq!(
            refunded(
                SpokeAsset::Tao,
                bought - (EXISTENTIAL_DEPOSIT - 1),
                Destination::NativeTao { to: ALICE }
            ),
            Error::<Test>::ReserveDust.into()
        );
        settled(
            SpokeAsset::Tao,
            bought - EXISTENTIAL_DEPOSIT,
            Destination::NativeTao { to: ALICE },
        );
        assert_eq!(
            TaoSupply::<Test>::get(BASE),
            TaoBalance::from(EXISTENTIAL_DEPOSIT)
        );
        assert_invariants();
    });
}

// ---------------------------------------------------------------- alpha tokens

#[test]
fn opening_a_market_opens_the_index_for_its_vault() {
    new_test_ext().execute_with(|| {
        assert_noop!(
            Rails::open_market(RuntimeOrigin::root(), netuid(CHUTES), UNIT.into()),
            Error::<Test>::SubnetUnknown
        );
        add_subnet(CHUTES, "Chutes");
        assert_ok!(Rails::open_market(
            RuntimeOrigin::root(),
            netuid(CHUTES),
            (500 * UNIT).into()
        ));

        let vault = Rails::vault_account(netuid(CHUTES));
        assert_eq!(OpenedIndexes::get(netuid(CHUTES)), Some(vault.clone()));
        assert_ne!(vault, Rails::vault_account(netuid(LIUM)));
        assert_ne!(vault, Rails::hub_account());
        assert_ne!(vault, Rails::reserve_account());
        let market = Markets::<Test>::get(netuid(CHUTES)).unwrap();
        assert_eq!((market.generation, market.shares), (1, 0));
        assert_eq!(market.cap, (500 * UNIT).into());
        assert_noop!(
            Rails::open_market(RuntimeOrigin::root(), netuid(CHUTES), UNIT.into()),
            Error::<Test>::MarketExists
        );

        assert_ok!(Rails::set_market_cap(
            RuntimeOrigin::root(),
            netuid(CHUTES),
            (900 * UNIT).into()
        ));
        assert_eq!(
            Markets::<Test>::get(netuid(CHUTES)).unwrap().cap,
            (900 * UNIT).into()
        );
        assert_noop!(
            Rails::set_market_cap(RuntimeOrigin::root(), netuid(LIUM), UNIT.into()),
            Error::<Test>::MarketUnknown
        );
    });
}

#[test]
fn listing_names_the_token_from_the_subnet_identity() {
    new_test_ext().execute_with(|| {
        add_subnet(LIUM, "lium.io");
        assert_noop!(
            Rails::list_market(RuntimeOrigin::root(), netuid(LIUM), BASE, wallet()),
            Error::<Test>::MarketUnknown
        );
        assert_ok!(Rails::open_market(
            RuntimeOrigin::root(),
            netuid(LIUM),
            UNIT.into()
        ));
        assert_ok!(Rails::list_market(
            RuntimeOrigin::root(),
            netuid(LIUM),
            BASE,
            wallet()
        ));

        let token = TokenInfo {
            address: wallet(),
            name: b"lium.io".to_vec().try_into().unwrap(),
            symbol: b"LIUM".to_vec().try_into().unwrap(),
        };
        assert_eq!(
            Tokens::<Test>::get(BASE, SpokeAsset::Alpha(netuid(LIUM))),
            Some(token.clone())
        );
        // The spoke is told to create exactly that token.
        assert_eq!(
            flush()[0].message,
            Message::RegisterToken {
                netuid: netuid(LIUM),
                token: wallet(),
                name: token.name,
                symbol: token.symbol,
            }
        );
        // Once per spoke; another spoke lists it on its own.
        assert_noop!(
            Rails::list_market(RuntimeOrigin::root(), netuid(LIUM), BASE, wallet()),
            Error::<Test>::MarketExists
        );
        assert_ok!(Rails::list_market(
            RuntimeOrigin::root(),
            netuid(LIUM),
            ETHEREUM,
            wallet()
        ));
        assert_eq!(flush()[0].bridge, ETHEREUM);
    });
}

// ---------------------------------------------------------------- what cannot be paid

#[test]
fn an_order_for_the_zero_address_is_refunded_to_its_payer() {
    new_test_ext().execute_with(|| {
        open_market(CHUTES, "Chutes");
        add_service(CHUTES, SERVICE);
        let nobody = H160::zero();
        // A token cannot be minted to the zero address: the hub must not settle these.
        for destination in [
            Destination::Tao { to: nobody },
            Destination::Alpha {
                netuid: netuid(CHUTES),
                to: nobody,
            },
            Destination::Credits {
                netuid: netuid(CHUTES),
                service: SERVICE,
                beneficiary: nobody,
            },
        ] {
            assert_eq!(
                refunded(SpokeAsset::Stable, 100 * UNIT, destination),
                Error::<Test>::InvalidRoute.into()
            );
        }
        // Nothing was minted, staked or reserved for them.
        assert_eq!(outstanding(BASE), 0);
        assert_eq!(Markets::<Test>::get(netuid(CHUTES)).unwrap().shares, 0);
        assert_invariants();
    });
}

#[test]
fn an_exit_to_the_zero_address_fails() {
    new_test_ext().execute_with(|| {
        fund_tao(ALICE, 10 * UNIT);
        assert_noop!(
            Rails::bridge_out(
                RuntimeOrigin::signed(ALICE),
                BASE,
                NativeAsset::Tao,
                UNIT,
                Destination::Stable { to: H160::zero() },
                0
            ),
            Error::<Test>::InvalidRoute
        );
    });
}

#[test]
fn the_stablecoin_minimum_counts_what_the_spoke_can_pay() {
    new_test_ext().execute_with(|| {
        // One unit of USDC is 1,000 of the hub's units.
        settled(
            SpokeAsset::Stable,
            500 * UNIT,
            Destination::NativeTusd { to: ALICE },
        );
        let pay = |amount: u64, min_out: u64| {
            Rails::bridge_out(
                RuntimeOrigin::signed(ALICE),
                BASE,
                NativeAsset::Tusd,
                amount,
                Destination::Stable { to: wallet() },
                min_out,
            )
        };

        // 2.000000999 tUSD pays 2.000000 USDC. A minimum above that is not met, though
        // the tUSD itself is more.
        assert_noop!(
            pay(2 * UNIT + 999, 2 * UNIT + 1),
            Error::<Test>::SlippageExceeded
        );
        // Less than one unit of the stablecoin would pay nothing. tUSD does not move in
        // amounts that small (its minimum balance is far above one unit), so such a
        // payment stops before the hub's own check for a payout of nothing.
        assert_noop!(pay(999, 0), pallet_tusd::Error::<Test>::BelowMinimumBalance);

        assert_ok!(pay(2 * UNIT + 999, 2 * UNIT));
        // The spoke is told to pay whole units. All of the tUSD is burned: the part
        // below one unit stays in the vault as the stablecoin.
        assert_eq!(
            flush()[0].message,
            Message::Credit {
                asset: SpokeAsset::Stable,
                to: wallet(),
                amount: 2 * UNIT,
                order: None,
                refund: false,
                rate: 0,
            }
        );
        assert_eq!(outstanding(BASE), 500 * UNIT - (2 * UNIT + 999));
        assert_invariants();
    });
}

// ---------------------------------------------------------------- who pays for a message

/// The most the hub pays for one message to a spoke in these tests.
const RESERVE: u64 = UNIT / 100;

/// What the account of `bridge` can pay the transport.
fn budget(bridge: u32) -> u64 {
    Rails::message_budget(bridge).into()
}

fn gateway_tao() -> u64 {
    Balances::free_balance(GATEWAY).into()
}

/// The accounts of both bridges exist and can pay nothing: they hold the existential
/// deposit, which an account keeps.
fn open_message_accounts() {
    for bridge in SPOKES {
        fund_tao(Rails::message_account(bridge), EXISTENTIAL_DEPOSIT);
        assert_eq!(budget(bridge), 0);
    }
}

/// The hub sets `reserve` aside for each message to the spoke of `bridge`, and pays at
/// most `ceiling` for one.
fn set_terms(bridge: u32, reserve: u64, ceiling: u64) {
    assert_ok!(Rails::set_message_reserve(
        RuntimeOrigin::root(),
        bridge,
        reserve.into(),
        ceiling.into()
    ));
}

/// The hub sets `reserve` aside for each message, and pays no more than that for one.
fn set_reserve(bridge: u32, reserve: u64) {
    set_terms(bridge, reserve, reserve);
}

fn limit(bridge: u32) -> u64 {
    Rails::message_limit(bridge).into()
}

/// Alice pays `amount` into the account of `bridge`.
fn fund(bridge: u32, amount: u64) {
    // More than she pays in, so that her account stays alive.
    fund_tao(ALICE, amount + UNIT);
    assert_ok!(Rails::fund_messages(
        RuntimeOrigin::signed(ALICE),
        bridge,
        amount.into()
    ));
}

/// The gateway takes the head of the queue of `bridge` and pays the transport `charge` for
/// it, as the gateway contract does in one transaction. The gateway keeps nothing.
fn send_head(bridge: u32, charge: u64) -> sp_runtime::DispatchResult {
    let (_, _, outbound) = Rails::next_outbound(bridge).expect("a message waits");
    send_message(bridge, outbound.nonce, charge)
}

/// The gateway takes message `nonce` of `bridge`, the head of the queue or a deferred
/// one, and pays the transport `charge` for it.
fn send_message(bridge: u32, nonce: u64, charge: u64) -> sp_runtime::DispatchResult {
    Rails::ack_outbound(RuntimeOrigin::signed(GATEWAY), bridge, nonce, charge.into())?;
    assert_ok!(Balances::burn(
        RuntimeOrigin::signed(GATEWAY),
        charge.into(),
        false
    ));
    assert_eq!(gateway_tao(), EXISTENTIAL_DEPOSIT);
    Ok(())
}

/// The gateway defers the head of the queue of `bridge`. Returns its nonce.
fn defer_head(bridge: u32) -> u64 {
    let (_, _, outbound) = Rails::next_outbound(bridge).expect("a message waits");
    assert_ok!(Rails::defer_outbound(
        RuntimeOrigin::signed(GATEWAY),
        bridge,
        outbound.nonce
    ));
    outbound.nonce
}

fn to_canonical_tao() -> Order<AccountId> {
    order(
        SpokeAsset::Stable,
        400 * UNIT,
        Destination::Tao { to: wallet() },
    )
}

#[test]
fn a_bridge_takes_no_order_before_its_reserve_is_set() {
    new_test_ext().execute_with(|| {
        MessageReserve::<Test>::remove(BASE);
        let to_account = order(
            SpokeAsset::Stable,
            100 * UNIT,
            Destination::NativeTusd { to: ALICE },
        );
        // Not even an order that needs no answer if it settles: what a message may cost is
        // decided first.
        assert_noop!(
            execute(to_account.clone()),
            Error::<Test>::MessagesNotFunded
        );
        fund_tao(ALICE, 10 * UNIT);
        assert_noop!(
            Rails::bridge_out(
                RuntimeOrigin::signed(ALICE),
                BASE,
                NativeAsset::Tao,
                UNIT,
                Destination::Tao { to: wallet() },
                0
            ),
            Error::<Test>::MessagesNotFunded
        );

        // Zero is a decision too: the hub pays this transport nothing.
        set_reserve(BASE, 0);
        System::assert_last_event(
            Event::MessageReserveSet {
                bridge: BASE,
                reserve: 0.into(),
                ceiling: 0.into(),
            }
            .into(),
        );
        assert_ok!(execute(to_account));
        // Only for a bridge that exists.
        assert_noop!(
            Rails::set_message_reserve(RuntimeOrigin::root(), 7, 0.into(), 0.into()),
            Error::<Test>::BridgeDisabled
        );
        // The most the hub pays for a message is not less than what it sets aside for one.
        assert_noop!(
            Rails::set_message_reserve(RuntimeOrigin::root(), BASE, 2.into(), 1.into()),
            Error::<Test>::CeilingBelowReserve
        );
    });
}

#[test]
fn anyone_pays_into_a_bridges_account() {
    new_test_ext().execute_with(|| {
        fund_tao(ALICE, 10 * UNIT);
        let account = Rails::message_account(BASE);
        assert_ne!(account, Rails::message_account(ETHEREUM));

        assert_ok!(Rails::fund_messages(
            RuntimeOrigin::signed(ALICE),
            BASE,
            (2 * RESERVE).into()
        ));
        System::assert_last_event(
            Event::MessagesFunded {
                who: ALICE,
                bridge: BASE,
                amount: (2 * RESERVE).into(),
            }
            .into(),
        );
        // The TAO is in the bridge's own account. The gateway got nothing.
        assert_eq!(
            Balances::free_balance(account),
            TaoBalance::from(2 * RESERVE)
        );
        assert_eq!(
            Balances::free_balance(ALICE),
            TaoBalance::from(10 * UNIT - 2 * RESERVE)
        );
        assert_eq!(gateway_tao(), EXISTENTIAL_DEPOSIT);
        assert_eq!(budget(ETHEREUM), 0);
        // The account keeps the existential deposit: the rest pays for messages.
        assert_eq!(budget(BASE), 2 * RESERVE - EXISTENTIAL_DEPOSIT);

        // Not for a bridge that does not exist: nothing could ever use it.
        assert_noop!(
            Rails::fund_messages(RuntimeOrigin::signed(ALICE), 7, RESERVE.into()),
            Error::<Test>::BridgeDisabled
        );
        // Not more than the payer has.
        assert_noop!(
            Rails::fund_messages(RuntimeOrigin::signed(BOB), BASE, RESERVE.into()),
            sp_runtime::TokenError::FundsUnavailable
        );
        // Not so little that the account could not exist.
        assert_noop!(
            Rails::fund_messages(
                RuntimeOrigin::signed(ALICE),
                ETHEREUM,
                (EXISTENTIAL_DEPOSIT - 1).into()
            ),
            sp_runtime::TokenError::BelowMinimum
        );
        // Paying in needs no gateway: the TAO is the hub's.
        Gateway::<Test>::kill();
        assert_ok!(Rails::fund_messages(
            RuntimeOrigin::signed(ALICE),
            BASE,
            RESERVE.into()
        ));
    });
}

#[test]
fn a_bridges_account_holds_the_reserve_for_each_queued_message() {
    new_test_ext().execute_with(|| {
        open_message_accounts();
        set_reserve(BASE, RESERVE);
        fund(BASE, 2 * RESERVE);

        // An order that settles with an answer and one that is refunded: two messages
        // wait. Nothing leaves the account until a message leaves.
        assert_ok!(execute(to_canonical_tao()));
        let mut refund = to_canonical_tao();
        refund.min_out = u64::MAX;
        assert_ok!(execute(refund));
        assert_eq!(outbox().len(), 2);
        assert_eq!(budget(BASE), 2 * RESERVE);

        // The account does not hold the reserve for a third message. Neither a settlement
        // nor a refund is taken: nothing changes, the order's nonce stays the next one,
        // and the transport delivers the order again later.
        let third = to_canonical_tao();
        assert_noop!(execute(third.clone()), Error::<Test>::MessagesNotFunded);
        let mut refund = third.clone();
        refund.min_out = u64::MAX;
        assert_noop!(execute(refund), Error::<Test>::MessagesNotFunded);
        // Every order has an answer if it fails, so the rule is the same for all.
        assert_noop!(
            execute(order(
                SpokeAsset::Stable,
                100 * UNIT,
                Destination::NativeTusd { to: ALICE },
            )),
            Error::<Test>::MessagesNotFunded
        );
        assert_eq!(NextOrder::<Test>::get(BASE), third.nonce);

        // One unit short is short.
        fund(BASE, RESERVE - 1);
        assert_noop!(execute(third.clone()), Error::<Test>::MessagesNotFunded);
        fund(BASE, 1);
        assert_ok!(execute(third));
        assert_eq!(outbox().len(), 3);
        assert_invariants();
    });
}

#[test]
fn a_bridges_account_keeps_the_existential_deposit() {
    new_test_ext().execute_with(|| {
        set_reserve(BASE, RESERVE);
        // The first payment into an empty account: the existential deposit stays in it.
        fund(BASE, RESERVE);
        assert_eq!(budget(BASE), RESERVE - EXISTENTIAL_DEPOSIT);
        assert_noop!(
            execute(to_canonical_tao()),
            Error::<Test>::MessagesNotFunded
        );
        fund(BASE, EXISTENTIAL_DEPOSIT);
        assert_ok!(execute(to_canonical_tao()));

        // The message leaves at the full reserve, and the account lives on.
        assert_ok!(send_head(BASE, RESERVE));
        assert_eq!(budget(BASE), 0);
        assert_eq!(
            Balances::free_balance(Rails::message_account(BASE)),
            TaoBalance::from(EXISTENTIAL_DEPOSIT)
        );
        assert_invariants();
    });
}

#[test]
fn the_hub_pays_what_the_transport_charges_up_to_the_reserve() {
    new_test_ext().execute_with(|| {
        open_message_accounts();
        set_reserve(BASE, RESERVE);
        fund(BASE, 2 * RESERVE);
        assert_ok!(execute(to_canonical_tao()));
        assert_ok!(execute(to_canonical_tao()));

        // The transport charges less than the reserve: the rest stays in the account.
        assert_ok!(send_head(BASE, RESERVE - 3));
        System::assert_has_event(
            Event::MessageDispatched {
                bridge: BASE,
                nonce: 0,
                paid: (RESERVE - 3).into(),
            }
            .into(),
        );
        assert_eq!(budget(BASE), RESERVE + 3);

        // It charges more than the reserve. The account holds that much, but the hub pays
        // no more than the reserve for one message: nothing changes, and the message
        // waits for a lower charge or for someone who pays the difference.
        assert_noop!(send_head(BASE, RESERVE + 1), Error::<Test>::FeeAboveLimit);
        assert_eq!(Rails::next_outbound(BASE).unwrap().2.nonce, 1);
        assert_ok!(send_head(BASE, RESERVE));
        assert_eq!(budget(BASE), 3);
        assert_eq!(Rails::next_outbound(BASE), None);
        assert_invariants();
    });
}

#[test]
fn a_charge_above_the_reserve_is_paid_from_what_the_account_can_spare() {
    new_test_ext().execute_with(|| {
        open_message_accounts();
        // The hub sets one reserve aside for a message, and pays up to three for one.
        set_terms(BASE, RESERVE, 3 * RESERVE);
        fund(BASE, 3 * RESERVE);
        for _ in 0..3 {
            assert_ok!(execute(to_canonical_tao()));
        }

        // The transport's charge rises to two reserves. The account holds the reserves of
        // the three messages and nothing to spare: the first message waits, and the queue
        // with it. Nothing changes.
        assert_eq!(limit(BASE), RESERVE);
        assert_noop!(send_head(BASE, 2 * RESERVE), Error::<Test>::FeeAboveLimit);

        // Anyone pays in. The whole queue drains at the higher charge: each message uses
        // its own reserve, and the rest comes from what the account can spare.
        fund(BASE, 3 * RESERVE);
        assert_eq!(limit(BASE), 3 * RESERVE);
        for left in [2, 1, 0] {
            assert_ok!(send_head(BASE, 2 * RESERVE));
            assert_eq!(budget(BASE), left * 2 * RESERVE);
            assert_invariants();
        }
        assert_eq!(Rails::next_outbound(BASE), None);
    });
}

#[test]
fn a_dear_message_never_takes_the_reserve_of_another() {
    new_test_ext().execute_with(|| {
        open_message_accounts();
        set_terms(BASE, RESERVE, 3 * RESERVE);
        // Two reserves for two messages, and half a reserve to spare.
        fund(BASE, 2 * RESERVE + RESERVE / 2);
        assert_ok!(execute(to_canonical_tao()));
        assert_ok!(execute(to_canonical_tao()));

        // The first message can cost its reserve and what is spare, and no more.
        assert_eq!(limit(BASE), RESERVE + RESERVE / 2);
        assert_noop!(
            send_head(BASE, RESERVE + RESERVE / 2 + 1),
            Error::<Test>::FeeAboveLimit
        );
        assert_ok!(send_head(BASE, RESERVE + RESERVE / 2));
        // The second still has its whole reserve.
        assert_eq!(limit(BASE), RESERVE);
        assert_ok!(send_head(BASE, RESERVE));

        // Above the ceiling the hub pays nothing, whatever the account can spare.
        fund(BASE, 100 * RESERVE);
        assert_ok!(execute(to_canonical_tao()));
        assert_eq!(limit(BASE), 3 * RESERVE);
        assert_noop!(
            send_head(BASE, 3 * RESERVE + 1),
            Error::<Test>::FeeAboveLimit
        );
        assert_ok!(send_head(BASE, 3 * RESERVE));
        assert_invariants();
    });
}

#[test]
fn a_deferred_message_keeps_its_reserve_and_the_ones_behind_it_leave() {
    new_test_ext().execute_with(|| {
        open_message_accounts();
        set_reserve(BASE, RESERVE);
        fund(BASE, 3 * RESERVE);
        for _ in 0..3 {
            assert_ok!(execute(to_canonical_tao()));
        }

        // Only the head is deferred, and only by the gateway or the admin.
        assert_noop!(
            Rails::defer_outbound(RuntimeOrigin::signed(GATEWAY), BASE, 1),
            Error::<Test>::NotOutboxHead
        );
        assert_noop!(
            Rails::defer_outbound(RuntimeOrigin::signed(ALICE), BASE, 0),
            Error::<Test>::NotGateway
        );

        // The transport charges more for the first message than is paid: the gateway
        // defers it. It has not left, and it keeps its reserve.
        assert_eq!(defer_head(BASE), 0);
        System::assert_last_event(
            Event::MessageDeferred {
                bridge: BASE,
                nonce: 0,
            }
            .into(),
        );
        assert_eq!(Deferred::<Test>::get(BASE), 1);
        assert_eq!(Rails::waiting(BASE), 3);
        assert_eq!(budget(BASE), 3 * RESERVE);
        assert!(Rails::outbound(BASE, 0).is_some());
        assert_invariants();

        // The messages behind it leave.
        assert_eq!(Rails::next_outbound(BASE).unwrap().2.nonce, 1);
        assert_ok!(send_head(BASE, RESERVE));
        assert_ok!(send_head(BASE, RESERVE));
        assert_eq!(Rails::next_outbound(BASE), None);
        assert_eq!(Rails::waiting(BASE), 1);

        // Its reserve is its own: the bridge takes no new order on it.
        assert_eq!(budget(BASE), RESERVE);
        assert_noop!(
            execute(to_canonical_tao()),
            Error::<Test>::MessagesNotFunded
        );

        // The deferred message leaves when the gateway sends it: later than
        // the ones behind it, and once.
        assert_noop!(
            send_message(BASE, 0, RESERVE + 1),
            Error::<Test>::FeeAboveLimit
        );
        assert_ok!(send_message(BASE, 0, RESERVE));
        assert_eq!(Deferred::<Test>::get(BASE), 0);
        assert_eq!(Rails::waiting(BASE), 0);
        assert_eq!(budget(BASE), 0);
        assert_eq!(Rails::outbound(BASE, 0), None);
        assert_noop!(send_message(BASE, 0, 0), Error::<Test>::NotOutboxHead);
        assert_invariants();
    });
}

#[test]
fn the_admin_defers_a_message_that_the_transport_does_not_take() {
    new_test_ext().execute_with(|| {
        for _ in 0..2 {
            settled(
                SpokeAsset::Stable,
                400 * UNIT,
                Destination::Tao { to: wallet() },
            );
        }
        // A paused hub moves no message.
        assert_ok!(Rails::set_paused(RuntimeOrigin::signed(GUARDIAN), true));
        assert_noop!(
            Rails::defer_outbound(RuntimeOrigin::root(), BASE, 0),
            Error::<Test>::Paused
        );
        assert_eq!(Rails::outbound(BASE, 0), None);
        assert_ok!(Rails::set_paused(RuntimeOrigin::root(), false));

        assert_ok!(Rails::defer_outbound(RuntimeOrigin::root(), BASE, 0));
        // The message behind it is the gateway's next, and the first is still there.
        assert_eq!(flush().len(), 1);
        assert_eq!(Rails::outbound(BASE, 0).unwrap().2.nonce, 0);
        assert_ok!(send_message(BASE, 0, 0));
        assert_invariants();
    });
}

#[test]
fn deferred_messages_take_no_place_in_the_queue() {
    new_test_ext().execute_with(|| {
        let to_spoke = || {
            order(
                SpokeAsset::Stable,
                40 * UNIT,
                Destination::Tao { to: wallet() },
            )
        };
        // Twice as many messages as the queue holds, each deferred as it comes: the
        // queue is empty, and the hub still takes orders.
        for _ in 0..2 * MAX_OUTBOX {
            assert_ok!(execute(to_spoke()));
            defer_head(BASE);
        }
        assert_eq!(Rails::next_outbound(BASE), None);
        assert_eq!(Rails::waiting(BASE), u64::from(2 * MAX_OUTBOX));
        assert_invariants();

        // The queue holds its own number of messages on top of them, and no more.
        for _ in 0..MAX_OUTBOX {
            assert_ok!(execute(to_spoke()));
        }
        assert_noop!(execute(to_spoke()), Error::<Test>::OutboxFull);
        // One message of the queue leaves, or is deferred: the hub takes the next order.
        assert_ok!(send_head(BASE, 0));
        assert_ok!(execute(to_spoke()));
        defer_head(BASE);
        assert_ok!(execute(to_spoke()));
        assert_invariants();
    });
}

#[test]
fn a_congested_bridge_refunds_orders_and_refuses_exits() {
    new_test_ext().execute_with(|| {
        settled(
            SpokeAsset::Stable,
            500 * UNIT,
            Destination::NativeTusd { to: ALICE },
        );
        let exit = || {
            Rails::bridge_out(
                RuntimeOrigin::signed(ALICE),
                BASE,
                NativeAsset::Tusd,
                100 * UNIT,
                Destination::Stable { to: wallet() },
                0,
            )
        };
        let to_spoke = Destination::Tao { to: wallet() };

        // Orders settle while nobody pays what the transport charges for their answers:
        // each answer is deferred. One fewer than the limit, and the bridge still pays.
        for _ in 1..CONGESTED_AT {
            settled(SpokeAsset::Stable, 400 * UNIT, to_spoke.clone());
            defer_head(BASE);
        }
        assert!(!Rails::congested(BASE));
        assert_ok!(exit());
        defer_head(BASE);
        assert!(Rails::congested(BASE));

        // The hub still takes an order for the spoke, and refunds it: nothing is settled,
        // and the payer gets back what the portal took.
        let supply = TaoSupply::<Test>::get(BASE);
        let minted = outstanding(BASE);
        assert_eq!(
            refunded(SpokeAsset::Stable, 400 * UNIT, to_spoke.clone()),
            Error::<Test>::Congested.into()
        );
        assert_eq!(TaoSupply::<Test>::get(BASE), supply);
        assert_eq!(outstanding(BASE), minted);
        // A quote says so before anyone pays.
        assert_eq!(
            Rails::quote_order(BASE, SpokeAsset::Stable, 400 * UNIT, to_spoke.clone()),
            Err(Error::<Test>::Congested.into())
        );

        // An exit fails as a whole: the account keeps its funds.
        assert_noop!(exit(), Error::<Test>::Congested);

        // An order that is paid out on Bittensor needs no message. It settles.
        settled(
            SpokeAsset::Stable,
            250 * UNIT,
            Destination::NativeTusd { to: BOB },
        );

        // The refund is deferred too. The hub goes on taking orders and refunding them:
        // deferred messages take no place in the queue.
        defer_head(BASE);
        for _ in 0..2 * MAX_OUTBOX {
            refunded(SpokeAsset::Stable, 40 * UNIT, to_spoke.clone());
            defer_head(BASE);
        }

        // Deferred messages leave until fewer than the limit are left: the bridge makes
        // payments again.
        let deferred = u64::from(Deferred::<Test>::get(BASE));
        for nonce in 0..=deferred - u64::from(CONGESTED_AT) {
            assert!(Rails::congested(BASE));
            assert_ok!(send_message(BASE, nonce, 0));
        }
        assert!(!Rails::congested(BASE));
        settled(SpokeAsset::Stable, 400 * UNIT, to_spoke);
        assert_ok!(exit());
        assert_invariants();
    });
}

#[test]
fn the_reserve_rises_only_if_the_account_covers_the_queue() {
    new_test_ext().execute_with(|| {
        open_message_accounts();
        set_reserve(BASE, RESERVE);
        fund(BASE, 2 * RESERVE);
        assert_ok!(execute(to_canonical_tao()));
        assert_ok!(execute(to_canonical_tao()));

        // Two messages wait, each with one reserve behind it. A higher reserve would
        // promise more for them than the account holds.
        assert_noop!(
            Rails::set_message_reserve(
                RuntimeOrigin::root(),
                BASE,
                (RESERVE + 1).into(),
                (RESERVE + 1).into()
            ),
            Error::<Test>::MessagesNotFunded
        );
        fund(BASE, 2);
        set_reserve(BASE, RESERVE + 1);
        // Both can now leave at the higher charge.
        assert_ok!(send_head(BASE, RESERVE + 1));
        assert_ok!(send_head(BASE, RESERVE + 1));
        assert_eq!(budget(BASE), 0);

        // A lower reserve always holds.
        set_reserve(BASE, 1);
        assert_invariants();
    });
}

#[test]
fn one_bridge_never_spends_what_was_paid_in_for_another() {
    new_test_ext().execute_with(|| {
        open_message_accounts();
        // Both bridges pay for their messages. Only Ethereum's account was paid into.
        set_reserve(BASE, RESERVE);
        set_reserve(ETHEREUM, RESERVE);
        fund(ETHEREUM, 3 * RESERVE);
        let on_ethereum = |order: Order<AccountId>| {
            Rails::execute(
                RuntimeOrigin::signed(GATEWAY),
                ETHEREUM,
                portal(ETHEREUM),
                order,
            )
        };

        // Base takes no order, whatever Ethereum's account holds.
        let mut refund_on_base = to_canonical_tao();
        refund_on_base.min_out = u64::MAX;
        assert_noop!(execute(refund_on_base), Error::<Test>::MessagesNotFunded);

        // Ethereum's orders settle, and their answers are queued for Ethereum alone.
        let mut refund = to_canonical_tao();
        refund.nonce = NextOrder::<Test>::get(ETHEREUM);
        refund.min_out = u64::MAX;
        assert_ok!(on_ethereum(refund));
        let waiting = outbox();
        assert_eq!(waiting.len(), 1);
        assert_eq!(waiting[0].bridge, ETHEREUM);
        assert_eq!(Rails::next_outbound(BASE), None);

        // Base gets a payment of its own and queues an answer. Each message is paid for
        // from the account of its own bridge.
        fund(BASE, RESERVE);
        assert_ok!(execute(to_canonical_tao()));
        assert_ok!(send_head(ETHEREUM, RESERVE));
        assert_eq!(budget(ETHEREUM), 2 * RESERVE);
        assert_eq!(budget(BASE), RESERVE);
        assert_ok!(send_head(BASE, RESERVE - 1));
        assert_eq!(budget(ETHEREUM), 2 * RESERVE);
        assert_eq!(budget(BASE), 1);
        assert_invariants();
    });
}

#[test]
fn an_exit_pays_the_reserve_for_its_message() {
    new_test_ext().execute_with(|| {
        settled(
            SpokeAsset::Stable,
            500 * UNIT,
            Destination::NativeTusd { to: ALICE },
        );
        open_message_accounts();
        fund_tao(ALICE, 10 * UNIT);
        set_reserve(BASE, RESERVE);
        let exit = |who: AccountId, amount: u64| {
            Rails::bridge_out(
                RuntimeOrigin::signed(who),
                BASE,
                NativeAsset::Tusd,
                amount,
                Destination::Stable { to: wallet() },
                0,
            )
        };

        // Nobody paid into the bridge's account: the exit brings the reserve for its own
        // message, on top of what it sends.
        assert_ok!(exit(ALICE, 100 * UNIT));
        assert_eq!(Tusd::balance(&ALICE), TusdBalance::from(400 * UNIT));
        assert_eq!(
            Balances::free_balance(ALICE),
            TaoBalance::from(10 * UNIT - RESERVE)
        );
        assert_eq!(budget(BASE), RESERVE);
        assert_eq!(gateway_tao(), EXISTENTIAL_DEPOSIT);
        System::assert_has_event(
            Event::BridgedOut {
                who: ALICE,
                bridge: BASE,
                source: NativeAsset::Tusd,
                amount: 100 * UNIT,
                destination: Destination::Stable { to: wallet() },
                amount_out: 100 * UNIT,
                fee: RESERVE.into(),
            }
            .into(),
        );
        assert_eq!(outbox().len(), 1);

        // An account that cannot pay the reserve does not exit, and keeps what it has.
        assert_ok!(Tusd::transfer(
            RuntimeOrigin::signed(ALICE),
            BOB,
            TusdBalance::from(50 * UNIT)
        ));
        assert_noop!(
            exit(BOB, 50 * UNIT),
            sp_runtime::TokenError::FundsUnavailable
        );

        // The transport charges less than the reserve. What is left stays in the bridge's
        // account, and pays for a later message.
        assert_ok!(send_head(BASE, RESERVE / 4));
        assert_eq!(budget(BASE), RESERVE - RESERVE / 4);
        assert_ok!(exit(ALICE, 100 * UNIT));
        assert_eq!(budget(BASE), 2 * RESERVE - RESERVE / 4);
        assert_invariants();
    });
}

#[test]
fn a_new_gateway_is_paid_from_the_same_accounts() {
    new_test_ext().execute_with(|| {
        open_message_accounts();
        set_reserve(BASE, RESERVE);
        fund(BASE, 2 * RESERVE);
        assert_ok!(execute(to_canonical_tao()));
        assert_ok!(execute(to_canonical_tao()));
        assert_ok!(send_head(BASE, RESERVE));

        // An account that does not exist cannot be the gateway: it could not hold what the
        // hub hands it.
        assert_noop!(
            Rails::set_gateway(RuntimeOrigin::root(), Some(BOB)),
            Error::<Test>::GatewayNotFunded
        );
        fund_tao(BOB, EXISTENTIAL_DEPOSIT);
        assert_ok!(Rails::set_gateway(RuntimeOrigin::root(), Some(BOB)));

        // Nothing has to move: the old gateway held none of the TAO for messages. The
        // message that waits is the new gateway's to send, paid for as before.
        assert_eq!(gateway_tao(), EXISTENTIAL_DEPOSIT);
        assert_eq!(budget(BASE), RESERVE);
        assert_noop!(
            Rails::ack_outbound(RuntimeOrigin::signed(GATEWAY), BASE, 1, RESERVE.into()),
            Error::<Test>::NotGateway
        );
        assert_ok!(Rails::ack_outbound(
            RuntimeOrigin::signed(BOB),
            BASE,
            1,
            RESERVE.into()
        ));
        assert_eq!(
            Balances::free_balance(BOB),
            TaoBalance::from(EXISTENTIAL_DEPOSIT + RESERVE)
        );
        assert_eq!(budget(BASE), 0);
        assert_eq!(gateway_tao(), EXISTENTIAL_DEPOSIT);
        assert_invariants();
    });
}

/// Random orders, exits, payments into the bridges' accounts and changes of the reserve
/// and the ceiling on two bridges, and a gateway that pays a transport that charges what
/// it likes. After every step each account can pay what a separate count says: what was
/// paid in for that bridge, less what the transport charged for that bridge's messages.
/// Messages are deferred and leave later. Each account holds the reserve for every message
/// that has not left, a message leaves exactly when its charge is within what the hub
/// pays for it, and the gateway holds nothing. Deferred messages take no place in a
/// queue, and a bridge refunds orders and refuses exits exactly while it is congested.
#[test]
fn message_accounts_hold_under_random_operations() {
    new_test_ext().execute_with(|| {
        open_message_accounts();
        fund_tao(ALICE, 1_000 * UNIT);
        // Each bridge brings in the stablecoin that the exits below take out.
        for bridge in SPOKES {
            let mut deposit = order(
                SpokeAsset::Stable,
                50_000 * UNIT,
                Destination::NativeTusd { to: ALICE },
            );
            deposit.nonce = NextOrder::<Test>::get(bridge);
            assert_ok!(Rails::execute(
                RuntimeOrigin::signed(GATEWAY),
                bridge,
                portal(bridge),
                deposit
            ));
        }
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = move |bound: u64| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state % bound.max(1)
        };
        // What each account must be able to pay.
        let mut expected = [0u64; 2];
        // The deferred messages of each bridge that have not left.
        let mut deferred: [Vec<u64>; 2] = [Vec::new(), Vec::new()];
        // Messages queued, messages refused for lack of the reserve, messages sent,
        // messages held because the transport charged more than the hub pays, messages
        // sent at a charge above the reserve, messages sent after they were deferred,
        // exits refused because the bridge was congested, and orders refunded for that.
        let mut seen = [0u32; 8];

        for _ in 0..6_000 {
            let index = next(2) as usize;
            let bridge = SPOKES[index];
            let reserve: u64 = MessageReserve::<Test>::get(bridge).unwrap().into();
            let in_line = NextMessage::<Test>::get(bridge) - NextOutbound::<Test>::get(bridge);
            // Every message that has not left holds a reserve: in line or deferred.
            let queued = in_line + deferred[index].len() as u64;
            assert_eq!(Rails::waiting(bridge), queued);
            // What the hub must answer to one more message, given what the account holds.
            // Deferred messages take no place in the queue, and each holds a reserve.
            let room = |budget: u64| {
                if in_line >= u64::from(MAX_OUTBOX) {
                    Err(Error::<Test>::OutboxFull)
                } else if budget >= reserve * (queued + 1) {
                    Ok(())
                } else {
                    Err(Error::<Test>::MessagesNotFunded)
                }
            };
            // What the hub pays for one message now: its reserve, and above it what the
            // account holds beyond the reserves of the waiting messages, up to the ceiling.
            let pays = {
                let ceiling: u64 = MessageCeiling::<Test>::get(bridge).into();
                (reserve + budget(bridge) - reserve * queued).min(ceiling.max(reserve))
            };
            assert_eq!(limit(bridge), pays);
            match next(10) {
                // Someone pays in.
                0 => {
                    let amount = next(4 * RESERVE);
                    fund(bridge, amount);
                    expected[index] += amount;
                }
                // The gateway defers the head of the queue.
                8 if in_line > 0 => deferred[index].push(defer_head(bridge)),
                // The gateway sends a deferred message, at a charge that may be
                // more than the hub pays.
                9 if !deferred[index].is_empty() => {
                    let position = next(deferred[index].len() as u64) as usize;
                    let nonce = deferred[index][position];
                    let charge = next(2 * reserve + 2);
                    if charge <= pays {
                        assert_ok!(send_message(bridge, nonce, charge));
                        expected[index] -= charge;
                        deferred[index].remove(position);
                        seen[5] += 1;
                    } else {
                        assert_noop!(
                            send_message(bridge, nonce, charge),
                            Error::<Test>::FeeAboveLimit
                        );
                    }
                }
                // The admin changes the reserve and the ceiling: only to a reserve that the
                // account covers.
                1 => {
                    let reserve = next(2 * RESERVE);
                    let ceiling = reserve + next(3) * next(2 * RESERVE);
                    let change = || {
                        Rails::set_message_reserve(
                            RuntimeOrigin::root(),
                            bridge,
                            reserve.into(),
                            ceiling.into(),
                        )
                    };
                    if budget(bridge) >= reserve * queued {
                        assert_ok!(change());
                    } else {
                        assert_noop!(change(), Error::<Test>::MessagesNotFunded);
                    }
                }
                // An order. It is settled or refunded, and has one answer either way.
                2..=4 => {
                    let mut order = to_canonical_tao();
                    order.nonce = NextOrder::<Test>::get(bridge);
                    order.min_out = next(2) * u64::MAX;
                    let execute = || {
                        Rails::execute(
                            RuntimeOrigin::signed(GATEWAY),
                            bridge,
                            portal(bridge),
                            order.clone(),
                        )
                    };
                    match room(budget(bridge)) {
                        Ok(()) => {
                            assert_ok!(execute());
                            // The hub took the order. A congested bridge refunds it, and
                            // only a congested bridge gives that reason.
                            let congested = deferred[index].len() >= CONGESTED_AT as usize;
                            let outcome =
                                Receipts::<Test>::get(bridge, order.nonce).unwrap().outcome;
                            let refused = Outcome::Refunded {
                                error: Error::<Test>::Congested.into(),
                            };
                            assert_eq!(outcome == refused, congested, "{outcome:?}");
                            seen[if congested { 7 } else { 0 }] += 1;
                        }
                        Err(error) => {
                            assert_noop!(execute(), error);
                            seen[1] += 1;
                        }
                    }
                }
                // An exit. It pays the reserve for its message into the account first.
                5 => {
                    let exit = || {
                        Rails::bridge_out(
                            RuntimeOrigin::signed(ALICE),
                            bridge,
                            NativeAsset::Tusd,
                            10 * UNIT,
                            Destination::Stable { to: wallet() },
                            0,
                        )
                    };
                    // A congested bridge takes no exit, whatever its account holds.
                    let congested = deferred[index].len() >= CONGESTED_AT as usize;
                    assert_eq!(Rails::congested(bridge), congested);
                    let admitted = if congested {
                        Err(Error::<Test>::Congested)
                    } else {
                        room(budget(bridge) + reserve)
                    };
                    match admitted {
                        Ok(()) => {
                            assert_ok!(exit());
                            expected[index] += reserve;
                            seen[0] += 1;
                        }
                        Err(error) => {
                            assert_noop!(exit(), error);
                            seen[if congested { 6 } else { 1 }] += 1;
                        }
                    }
                }
                // The gateway takes the head of the queue and pays the transport, which now
                // and then charges more than the reserve.
                6 | 7 if in_line > 0 => {
                    let charge = next(2 * reserve + 2);
                    if charge <= pays {
                        assert_ok!(send_head(bridge, charge));
                        expected[index] -= charge;
                        seen[2] += 1;
                        seen[4] += u32::from(charge > reserve);
                    } else {
                        assert_noop!(send_head(bridge, charge), Error::<Test>::FeeAboveLimit);
                        seen[3] += 1;
                    }
                }
                _ => {}
            }
            assert_eq!([budget(BASE), budget(ETHEREUM)], expected);
            assert_eq!(gateway_tao(), EXISTENTIAL_DEPOSIT);
            assert_invariants();
        }
        assert!(seen.iter().all(|count| *count > 100), "{seen:?}");
    });
}

#[test]
fn the_registry_address_of_a_listed_alpha_token_can_be_corrected() {
    new_test_ext().execute_with(|| {
        add_subnet(CHUTES, "Chutes");
        let asset = SpokeAsset::Alpha(netuid(CHUTES));
        let correct = H160::repeat_byte(0xcc);
        // Only a token that was listed: listing is what has the spoke create it.
        assert_noop!(
            Rails::register_token(RuntimeOrigin::root(), BASE, asset, correct),
            Error::<Test>::TokenNotRegistered
        );
        open_market(CHUTES, "Chutes");
        let listed = Tokens::<Test>::get(BASE, asset).unwrap();
        assert_ne!(listed.address, correct);

        assert_ok!(Rails::register_token(
            RuntimeOrigin::root(),
            BASE,
            asset,
            correct
        ));

        // The address changes. The name stays, and the spoke is told nothing.
        assert_eq!(
            Tokens::<Test>::get(BASE, asset),
            Some(TokenInfo {
                address: correct,
                ..listed
            })
        );
        assert!(outbox().is_empty());
    });
}

#[test]
fn token_names_come_from_what_the_subnet_published() {
    new_test_ext().execute_with(|| {
        let identity = |raw: u16, name: &str| {
            add_subnet(raw, name);
            let (name, symbol) = Rails::token_identity(netuid(raw));
            (
                String::from_utf8(name.to_vec()).unwrap(),
                String::from_utf8(symbol.to_vec()).unwrap(),
            )
        };
        let pair = |name: &str, symbol: &str| (name.to_string(), symbol.to_string());
        assert_eq!(identity(64, "Chutes"), pair("Chutes", "CHUTES"));
        assert_eq!(identity(51, "lium.io"), pair("lium.io", "LIUM"));
        assert_eq!(
            identity(19, "blockmachine"),
            pair("blockmachine", "BLOCKMACHIN")
        );
        assert_eq!(identity(4, "  Targon v2"), pair("  Targon v2", "TARGON"));
        assert_eq!(identity(8, "τ-vanta"), pair("τ-vanta", "VANTA"));
        // Nothing usable: the netuid names it.
        assert_eq!(identity(7, ""), pair("Subnet 7", "SN7"));
        assert_eq!(identity(9, "τττ"), pair("Subnet 9", "SN9"));
        assert_eq!(identity(300, "\u{0}\u{7}"), pair("Subnet 300", "SN300"));
        // A long name is cut where it is still text.
        let long = "é".repeat(40);
        let (name, symbol) = identity(10, &format!("{long}x"));
        assert_eq!(name, "é".repeat(32));
        assert_eq!(symbol, "X");
    });
}

#[test]
fn stablecoin_buys_alpha_tokens_staked_by_the_vault() {
    new_test_ext().execute_with(|| {
        open_market(CHUTES, "Chutes");
        let tao: u64 = Pool::quote_tusd_for_tao((4_000 * UNIT).into())
            .unwrap()
            .into();

        let shares = settled(
            SpokeAsset::Stable,
            4_000 * UNIT,
            Destination::Alpha {
                netuid: netuid(CHUTES),
                to: wallet(),
            },
        );

        // The first shares are one alpha each.
        let vault = Rails::vault_account(netuid(CHUTES));
        let staked: u64 = MockStaking::staked(&vault, netuid(CHUTES)).into();
        assert_eq!(staked, tao * ALPHA_PER_TAO);
        assert_eq!(shares, staked);
        assert_eq!(Markets::<Test>::get(netuid(CHUTES)).unwrap().shares, shares);
        assert_eq!(AlphaSupply::<Test>::get(netuid(CHUTES), BASE), shares);
        assert_eq!(Rails::alpha_rate(netuid(CHUTES)), Some(RATE_SCALE));
        assert_eq!(
            flush()[0].message,
            Message::Credit {
                asset: SpokeAsset::Alpha(netuid(CHUTES)),
                to: wallet(),
                amount: shares,
                order: Some(0),
                refund: false,
                rate: RATE_SCALE,
            }
        );
        assert_invariants();
    });
}

#[test]
fn the_first_purchase_leaves_the_existential_deposit_in_the_vault() {
    new_test_ext().execute_with(|| {
        // A market whose vault account does not exist yet.
        add_subnet(CHUTES, "Chutes");
        assert_ok!(Rails::open_market(
            RuntimeOrigin::root(),
            netuid(CHUTES),
            (1_000_000 * UNIT).into()
        ));
        assert_ok!(Rails::list_market(
            RuntimeOrigin::root(),
            netuid(CHUTES),
            BASE,
            wallet()
        ));
        let vault = Rails::vault_account(netuid(CHUTES));
        let alpha = Destination::Alpha {
            netuid: netuid(CHUTES),
            to: wallet(),
        };
        let tao: u64 = Pool::quote_tusd_for_tao((4_000 * UNIT).into())
            .unwrap()
            .into();

        // Staking keeps the account that pays alive: the first buyer's shares are for that
        // much less TAO, once.
        let shares = settled(SpokeAsset::Stable, 4_000 * UNIT, alpha.clone());
        assert_eq!(shares, (tao - EXISTENTIAL_DEPOSIT) * ALPHA_PER_TAO);
        assert_eq!(
            Balances::free_balance(&vault),
            TaoBalance::from(EXISTENTIAL_DEPOSIT)
        );

        let tao: u64 = Pool::quote_tusd_for_tao((4_000 * UNIT).into())
            .unwrap()
            .into();
        let more = settled(SpokeAsset::Stable, 4_000 * UNIT, alpha);
        assert_eq!(more, tao * ALPHA_PER_TAO);
        assert_eq!(
            Balances::free_balance(&vault),
            TaoBalance::from(EXISTENTIAL_DEPOSIT)
        );
        assert_invariants();
    });
}

#[test]
fn a_share_is_worth_more_alpha_as_the_vault_earns() {
    new_test_ext().execute_with(|| {
        open_market(CHUTES, "Chutes");
        let alpha = Destination::Alpha {
            netuid: netuid(CHUTES),
            to: wallet(),
        };
        let first = settled(SpokeAsset::Stable, 4_000 * UNIT, alpha.clone());
        let staked: u64 =
            MockStaking::staked(&Rails::vault_account(netuid(CHUTES)), netuid(CHUTES)).into();

        // Emissions add a tenth to the vault. The shares stand still.
        accrue(CHUTES, staked / 10);
        assert_eq!(AlphaSupply::<Test>::get(netuid(CHUTES), BASE), first);
        let grown = Rails::alpha_rate(netuid(CHUTES)).unwrap();
        assert!(grown > RATE_SCALE * 109 / 100 && grown < RATE_SCALE * 11 / 10 + 1);

        // A later buyer pays the higher rate: the same dollars buy fewer shares, and the
        // first holder's shares are not diluted.
        let second = settled(SpokeAsset::Stable, 4_000 * UNIT, alpha);
        assert!(second < first * 92 / 100);
        let after = Rails::alpha_rate(netuid(CHUTES)).unwrap();
        assert!(after >= grown, "a purchase lowered the rate");
        let messages = flush();
        let Message::Credit { rate, .. } = messages[1].message else {
            panic!("no credit");
        };
        assert_eq!(rate, after);
        assert_invariants();
    });
}

#[test]
fn alpha_tokens_sell_for_the_stablecoin_with_their_earnings() {
    new_test_ext().execute_with(|| {
        open_market(CHUTES, "Chutes");
        let shares = settled(
            SpokeAsset::Stable,
            4_000 * UNIT,
            Destination::Alpha {
                netuid: netuid(CHUTES),
                to: wallet(),
            },
        );
        flush();
        let vault = Rails::vault_account(netuid(CHUTES));
        let staked: u64 = MockStaking::staked(&vault, netuid(CHUTES)).into();
        accrue(CHUTES, staked / 10);

        // Half the tokens are burned on Base.
        let paid = settled(
            SpokeAsset::Alpha(netuid(CHUTES)),
            shares / 2,
            Destination::Stable { to: wallet() },
        );

        // Half of 4,000 dollars plus a tenth, less the fees and price impact of two swaps.
        assert!(paid > 2_170 * UNIT && paid < 2_200 * UNIT, "{paid}");
        assert_eq!(
            AlphaSupply::<Test>::get(netuid(CHUTES), BASE),
            shares - shares / 2
        );
        let left: u64 = MockStaking::staked(&vault, netuid(CHUTES)).into();
        // Half of the vault is left, less what the virtual shares keep of the earnings.
        let half = staked * 11 / 10 / 2;
        assert!(left >= half && left - half < half / 10_000, "{left} {half}");
        assert_eq!(
            flush()[0].message,
            Message::Credit {
                asset: SpokeAsset::Stable,
                to: wallet(),
                amount: paid,
                order: Some(1),
                refund: false,
                rate: 0,
            }
        );
        // Selling does not move the rate against those who stay.
        assert!(Rails::alpha_rate(netuid(CHUTES)).unwrap() >= RATE_SCALE * 109 / 100);
        assert_invariants();
    });
}

#[test]
fn alpha_tokens_route_only_through_tao() {
    new_test_ext().execute_with(|| {
        open_market(CHUTES, "Chutes");
        open_market(LIUM, "lium.io");
        let chutes = settled(
            SpokeAsset::Stable,
            4_000 * UNIT,
            Destination::Alpha {
                netuid: netuid(CHUTES),
                to: wallet(),
            },
        );
        let pool = Pool::reserves();

        // CHUTES for LIUM: unstake to TAO, stake the TAO. The pool is not touched.
        let lium = settled(
            SpokeAsset::Alpha(netuid(CHUTES)),
            chutes,
            Destination::Alpha {
                netuid: netuid(LIUM),
                to: wallet(),
            },
        );

        assert_eq!(lium, chutes);
        assert_eq!(Pool::reserves(), pool);
        assert_eq!(AlphaSupply::<Test>::get(netuid(CHUTES), BASE), 0);
        assert_eq!(AlphaSupply::<Test>::get(netuid(LIUM), BASE), lium);
        assert_invariants();
    });
}

#[test]
fn alpha_purchases_respect_the_market() {
    new_test_ext().execute_with(|| {
        let chutes = Destination::Alpha {
            netuid: netuid(CHUTES),
            to: wallet(),
        };
        // No market.
        assert_eq!(
            refunded(SpokeAsset::Stable, 4_000 * UNIT, chutes.clone()),
            Error::<Test>::MarketUnknown.into()
        );
        // A market that is not listed on this spoke.
        add_subnet(CHUTES, "Chutes");
        assert_ok!(Rails::open_market(
            RuntimeOrigin::root(),
            netuid(CHUTES),
            (10 * UNIT).into()
        ));
        assert_eq!(
            refunded(SpokeAsset::Stable, 4_000 * UNIT, chutes.clone()),
            Error::<Test>::TokenNotRegistered.into()
        );
        assert_ok!(Rails::list_market(
            RuntimeOrigin::root(),
            netuid(CHUTES),
            BASE,
            wallet()
        ));
        flush();
        // A vault at its cap of 10 alpha: 4,000 dollars buy about 20.
        assert_eq!(
            refunded(SpokeAsset::Stable, 4_000 * UNIT, chutes.clone()),
            Error::<Test>::MarketCapExceeded.into()
        );
        settled(SpokeAsset::Stable, 1_000 * UNIT, chutes);
        // The refunds minted no tUSD and staked nothing.
        assert_eq!(outstanding(BASE), 1_000 * UNIT);
        assert_invariants();
    });
}

// ---------------------------------------------------------------- credits

fn credits(service: AccountId) -> Destination<AccountId> {
    Destination::Credits {
        netuid: netuid(LIUM),
        service,
        beneficiary: wallet(),
    }
}

#[test]
fn stablecoin_buys_service_credits_for_an_address() {
    new_test_ext().execute_with(|| {
        add_subnet(LIUM, "lium.io");
        add_service(LIUM, SERVICE);
        keep_vault_alive(LIUM);
        let tao: u64 = Pool::quote_tusd_for_tao((100 * UNIT).into())
            .unwrap()
            .into();

        let gamma = settled(SpokeAsset::Stable, 100 * UNIT, credits(SERVICE));

        // A hundred dollars buy a little under a hundred dollars of credit: the pool's fee
        // and the price impact are the difference.
        assert!(gamma > 99 * UNIT && gamma < 100 * UNIT, "{gamma}");
        assert_eq!(Spent::get((netuid(LIUM), SERVICE, wallet())), gamma);
        // Nothing is paid out anywhere: the credit is spent.
        assert!(outbox().is_empty());
        assert_eq!(
            Balances::free_balance(SUBNET_POOL),
            TaoBalance::from(UNIT + tao)
        );
        // The service is told which address paid, on which chain and in which order.
        assert_eq!(Payments::get(), vec![(wallet(), OriginChain::Evm(8453), 0)]);
        System::assert_has_event(
            Event::CreditsPurchased {
                bridge: BASE,
                nonce: Some(0),
                beneficiary: wallet(),
                netuid: netuid(LIUM),
                service: SERVICE,
                tao: tao.into(),
                alpha_burned: (tao * ALPHA_PER_TAO).into(),
                gamma: gamma.into(),
            }
            .into(),
        );
        assert_invariants();
    });
}

#[test]
fn credits_are_valued_at_the_pool_price_over_time_when_it_is_lower() {
    new_test_ext().execute_with(|| {
        add_subnet(LIUM, "lium.io");
        add_service(LIUM, SERVICE);
        keep_vault_alive(LIUM);
        // Someone sells TAO into the pool and holds its price a fifth below the oracle.
        fund_tao(BOB, 2_000 * UNIT);
        assert_ok!(Pool::swap_tao_for_tusd(
            RuntimeOrigin::signed(BOB),
            (1_180 * UNIT).into(),
            0.into()
        ));
        System::set_block_number(System::block_number() + 3 * u64::from(TWAP_PERIOD));
        let twap = Pool::twap_price().unwrap();
        assert!(twap < ORACLE_PRICE * 81 / 100 && twap > ORACLE_PRICE * 79 / 100);
        let tao: u64 = Pool::quote_tusd_for_tao((100 * UNIT).into())
            .unwrap()
            .into();

        let gamma = settled(SpokeAsset::Stable, 100 * UNIT, credits(SERVICE));

        // The cheap pool sold more TAO for the dollars, but each TAO counts at the pool's
        // price, not the oracle's: a hundred dollars still buy under a hundred of credit.
        assert_eq!(
            gamma,
            (u128::from(tao) * u128::from(twap) / u128::from(UNIT)) as u64
        );
        assert!(gamma < 100 * UNIT, "{gamma}");
        assert!(
            u128::from(tao) * u128::from(ORACLE_PRICE) / u128::from(UNIT) > 120 * u128::from(UNIT)
        );
        assert_invariants();
    });
}

#[test]
fn credits_need_an_established_pool_price() {
    new_test_ext().execute_with(|| {
        add_subnet(LIUM, "lium.io");
        add_service(LIUM, SERVICE);
        // A pool without a full period of history, as right after its first deposit.
        pallet_canonical_pool::Accumulator::<Test>::mutate(|accumulator| {
            let now = System::block_number() as u32;
            accumulator.updated_at = now;
            accumulator.reference = (now, accumulator.cumulative);
            accumulator.checkpoint = (now, accumulator.cumulative);
        });
        assert_eq!(Pool::twap_price(), None);
        assert_eq!(
            refunded(SpokeAsset::Stable, 100 * UNIT, credits(SERVICE)),
            Error::<Test>::TwapUnavailable.into()
        );
        assert_eq!(Spent::iter().count(), 0);
        assert_invariants();
    });
}

#[test]
fn credits_that_cannot_be_spent_are_refunded_in_full() {
    new_test_ext().execute_with(|| {
        add_subnet(LIUM, "lium.io");
        add_service(LIUM, SERVICE);
        let pool = Pool::reserves();

        // A service that does not exist.
        assert_eq!(
            refunded(SpokeAsset::Stable, 100 * UNIT, credits(STRANGER)),
            NO_SERVICE
        );
        // Less credit than the payer accepts.
        let mut order = order(SpokeAsset::Stable, 100 * UNIT, credits(SERVICE));
        order.min_out = 100 * UNIT;
        assert_ok!(execute(order));
        assert_eq!(
            Receipts::<Test>::get(BASE, 1).unwrap().outcome,
            Outcome::Refunded {
                error: Error::<Test>::SlippageExceeded.into()
            }
        );

        // Neither order left a trace: no tUSD, no swap, no credit.
        assert_eq!(outstanding(BASE), 0);
        assert_eq!(Pool::reserves(), pool);
        assert_eq!(Spent::iter().count(), 0);
        assert_eq!(outbox().len(), 2);
        assert_invariants();
    });
}

// ---------------------------------------------------------------- refunds

#[test]
fn a_failed_order_is_rolled_back_and_refunded() {
    new_test_ext().execute_with(|| {
        let pool = Pool::reserves();
        let mut order = order(
            SpokeAsset::Stable,
            4_000 * UNIT,
            Destination::Tao { to: wallet() },
        );
        order.min_out = 10 * UNIT;

        assert_ok!(execute(order));

        // Its nonce is used up, and nothing else happened on the hub.
        assert_eq!(NextOrder::<Test>::get(BASE), 1);
        assert_eq!(outstanding(BASE), 0);
        assert_eq!(Pool::reserves(), pool);
        assert_eq!(TaoSupply::<Test>::get(BASE), TaoBalance::from(0));
        assert_eq!(
            outbox(),
            vec![Outbound {
                bridge: BASE,
                nonce: 0,
                message: Message::Credit {
                    asset: SpokeAsset::Stable,
                    to: payer(),
                    amount: 4_000 * UNIT,
                    order: Some(0),
                    refund: true,
                    rate: 0,
                },
            }]
        );
        System::assert_has_event(
            Event::OrderRefunded {
                bridge: BASE,
                nonce: 0,
                payer: payer(),
                source: SpokeAsset::Stable,
                amount: 4_000 * UNIT,
                error: Error::<Test>::SlippageExceeded.into(),
            }
            .into(),
        );
        assert_invariants();
    });
}

#[test]
fn a_refused_sale_gives_the_tokens_back() {
    new_test_ext().execute_with(|| {
        open_market(CHUTES, "Chutes");
        let shares = settled(
            SpokeAsset::Stable,
            4_000 * UNIT,
            Destination::Alpha {
                netuid: netuid(CHUTES),
                to: wallet(),
            },
        );
        flush();
        let staked = MockStaking::staked(&Rails::vault_account(netuid(CHUTES)), netuid(CHUTES));

        // The seller wants more dollars than the tokens fetch.
        let mut order = order(
            SpokeAsset::Alpha(netuid(CHUTES)),
            shares,
            Destination::Stable { to: wallet() },
        );
        order.min_out = 4_000 * UNIT;
        assert_ok!(execute(order));

        // The vault still holds the alpha and the spoke mints the shares again.
        assert_eq!(
            MockStaking::staked(&Rails::vault_account(netuid(CHUTES)), netuid(CHUTES)),
            staked
        );
        assert_eq!(AlphaSupply::<Test>::get(netuid(CHUTES), BASE), shares);
        assert_eq!(
            flush()[0].message,
            Message::Credit {
                asset: SpokeAsset::Alpha(netuid(CHUTES)),
                to: payer(),
                amount: shares,
                order: Some(1),
                refund: true,
                rate: 0,
            }
        );
        assert_invariants();
    });
}

#[test]
fn late_and_pointless_orders_are_refunded() {
    new_test_ext().execute_with(|| {
        open_market(CHUTES, "Chutes");
        // The clock reads 1,000 seconds.
        let mut late = order(
            SpokeAsset::Stable,
            UNIT,
            Destination::NativeTusd { to: ALICE },
        );
        late.deadline = 999;
        assert_ok!(execute(late));
        assert_eq!(
            Receipts::<Test>::get(BASE, 0).unwrap().outcome,
            Outcome::Refunded {
                error: Error::<Test>::Expired.into()
            }
        );
        let mut on_time = order(
            SpokeAsset::Stable,
            UNIT,
            Destination::NativeTusd { to: ALICE },
        );
        on_time.deadline = 1_000;
        assert_ok!(execute(on_time));
        assert_eq!(Tusd::balance(&ALICE), TusdBalance::from(UNIT));

        // An order that asks for what it paid with.
        assert_eq!(
            refunded(
                SpokeAsset::Stable,
                UNIT,
                Destination::Stable { to: wallet() }
            ),
            Error::<Test>::InvalidRoute.into()
        );
        // An order of nothing, and one too small to become an account's balance.
        assert_eq!(
            settle(SpokeAsset::Stable, 0, Destination::NativeTusd { to: ALICE }),
            Outcome::Refunded {
                error: Error::<Test>::AmountTooLow.into()
            }
        );
        assert_eq!(
            refunded(
                SpokeAsset::Stable,
                1_000,
                Destination::NativeTusd { to: BOB }
            ),
            pallet_tusd::Error::<Test>::BelowMinimumBalance.into()
        );
        assert_invariants();
    });
}

// ---------------------------------------------------------------- outbox and receipts

#[test]
fn the_gateway_takes_a_bridges_messages_first_in_first_out() {
    new_test_ext().execute_with(|| {
        assert_eq!(Rails::next_outbound(BASE), None);
        for _ in 0..3 {
            settled(
                SpokeAsset::Stable,
                400 * UNIT,
                Destination::Tao { to: wallet() },
            );
        }
        assert_eq!(NextOutbound::<Test>::get(BASE), 0);
        assert_eq!(NextMessage::<Test>::get(BASE), 3);

        let (domain, sender, outbound) = Rails::next_outbound(BASE).unwrap();
        assert_eq!(
            (domain, sender, outbound.bridge, outbound.nonce),
            (BASE, portal(BASE), BASE, 0)
        );
        // Only the head leaves, and only through the gateway.
        assert_noop!(
            Rails::ack_outbound(RuntimeOrigin::signed(GATEWAY), BASE, 1, 0.into()),
            Error::<Test>::NotOutboxHead
        );
        assert_noop!(
            Rails::ack_outbound(RuntimeOrigin::signed(ALICE), BASE, 0, 0.into()),
            Error::<Test>::NotGateway
        );
        // Another bridge's queue is empty: nothing of Base's leaves through it.
        assert_noop!(
            Rails::ack_outbound(RuntimeOrigin::signed(GATEWAY), ETHEREUM, 0, 0.into()),
            Error::<Test>::NotOutboxHead
        );
        assert_ok!(Rails::ack_outbound(
            RuntimeOrigin::signed(GATEWAY),
            BASE,
            0,
            0.into()
        ));
        System::assert_last_event(
            Event::MessageDispatched {
                bridge: BASE,
                nonce: 0,
                paid: 0.into(),
            }
            .into(),
        );
        assert_noop!(
            Rails::ack_outbound(RuntimeOrigin::signed(GATEWAY), BASE, 0, 0.into()),
            Error::<Test>::NotOutboxHead
        );

        // A paused hub shows and hands over nothing.
        assert_ok!(Rails::set_paused(RuntimeOrigin::signed(GUARDIAN), true));
        assert_eq!(Rails::next_outbound(BASE), None);
        assert_noop!(
            Rails::ack_outbound(RuntimeOrigin::signed(GATEWAY), BASE, 1, 0.into()),
            Error::<Test>::Paused
        );
        assert_ok!(Rails::set_paused(RuntimeOrigin::root(), false));

        let sent: Vec<u64> = flush().into_iter().map(|outbound| outbound.nonce).collect();
        assert_eq!(sent, vec![1, 2]);
        assert_eq!(Rails::next_outbound(BASE), None);
        assert_noop!(
            Rails::ack_outbound(RuntimeOrigin::signed(GATEWAY), BASE, 3, 0.into()),
            Error::<Test>::NotOutboxHead
        );
    });
}

#[test]
fn an_order_waits_while_its_bridges_queue_is_full() {
    new_test_ext().execute_with(|| {
        for _ in 0..MAX_OUTBOX {
            settled(
                SpokeAsset::Stable,
                40 * UNIT,
                Destination::Tao { to: wallet() },
            );
        }
        // Neither a settlement nor a refund can be answered: the order is not taken, and
        // the transport delivers it again once the gateway has made room.
        let settles = order(
            SpokeAsset::Stable,
            40 * UNIT,
            Destination::Tao { to: wallet() },
        );
        assert_noop!(execute(settles.clone()), Error::<Test>::OutboxFull);
        let mut refunds = settles.clone();
        refunds.min_out = u64::MAX;
        assert_noop!(execute(refunds), Error::<Test>::OutboxFull);
        // Every order has an answer if it fails, so the rule is the same for all.
        assert_noop!(
            execute(order(
                SpokeAsset::Stable,
                40 * UNIT,
                Destination::NativeTusd { to: ALICE },
            )),
            Error::<Test>::OutboxFull
        );
        // An order of another bridge goes through: its queue is its own.
        let mut on_ethereum = order(
            SpokeAsset::Stable,
            40 * UNIT,
            Destination::NativeTao { to: ALICE },
        );
        on_ethereum.nonce = NextOrder::<Test>::get(ETHEREUM);
        on_ethereum.min_out = u64::MAX;
        assert_ok!(Rails::execute(
            RuntimeOrigin::signed(GATEWAY),
            ETHEREUM,
            portal(ETHEREUM),
            on_ethereum
        ));
        assert_eq!(NextMessage::<Test>::get(ETHEREUM), 1);

        flush();
        let mut again = settles;
        again.nonce = NextOrder::<Test>::get(BASE);
        assert_ok!(execute(again));
        assert_invariants();
    });
}

#[test]
fn receipts_are_kept_for_recent_orders_only() {
    new_test_ext().execute_with(|| {
        for _ in 0..RECEIPTS_KEPT + 3 {
            settled(
                SpokeAsset::Stable,
                UNIT,
                Destination::NativeTusd { to: ALICE },
            );
        }
        let kept: Vec<u64> = {
            let mut nonces: Vec<u64> = Receipts::<Test>::iter_prefix(BASE)
                .map(|(nonce, _)| nonce)
                .collect();
            nonces.sort();
            nonces
        };
        assert_eq!(kept, (3..RECEIPTS_KEPT + 3).collect::<Vec<_>>());
        let receipt = Receipts::<Test>::get(BASE, RECEIPTS_KEPT + 2).unwrap();
        assert_eq!(receipt.block, System::block_number() as u32);
        assert_eq!(receipt.outcome, Outcome::Settled { amount_out: UNIT });
    });
}

#[test]
fn admin_passes_over_an_order_that_cannot_be_delivered() {
    new_test_ext().execute_with(|| {
        settled(
            SpokeAsset::Stable,
            UNIT,
            Destination::NativeTusd { to: ALICE },
        );
        // Order 1 cannot be decoded by the gateway, so order 2 waits behind it.
        let mut waiting = order(
            SpokeAsset::Stable,
            UNIT,
            Destination::NativeTusd { to: ALICE },
        );
        waiting.nonce = 2;
        assert_noop!(execute(waiting.clone()), Error::<Test>::OrderOutOfSequence);

        assert_noop!(
            Rails::skip_order(RuntimeOrigin::root(), BASE, 2),
            Error::<Test>::OrderOutOfSequence
        );
        assert_noop!(
            Rails::skip_order(RuntimeOrigin::root(), BASE, 0),
            Error::<Test>::OrderOutOfSequence
        );
        assert_ok!(Rails::skip_order(RuntimeOrigin::root(), BASE, 1));
        System::assert_last_event(
            Event::OrderSkipped {
                bridge: BASE,
                nonce: 1,
            }
            .into(),
        );
        assert_eq!(
            Receipts::<Test>::get(BASE, 1).unwrap().outcome,
            Outcome::Skipped
        );

        assert_ok!(execute(waiting));
        assert_eq!(Tusd::balance(&ALICE), TusdBalance::from(2 * UNIT));
    });
}

// ---------------------------------------------------------------- paying from Bittensor

#[test]
fn an_account_exits_tusd_to_the_stablecoin() {
    new_test_ext().execute_with(|| {
        // The dollars came in through Base.
        settled(
            SpokeAsset::Stable,
            500 * UNIT,
            Destination::NativeTusd { to: ALICE },
        );

        assert_ok!(Rails::bridge_out(
            RuntimeOrigin::signed(ALICE),
            BASE,
            NativeAsset::Tusd,
            200 * UNIT,
            Destination::Stable { to: wallet() },
            200 * UNIT
        ));

        assert_eq!(Tusd::balance(&ALICE), TusdBalance::from(300 * UNIT));
        assert_eq!(outstanding(BASE), 300 * UNIT);
        assert_eq!(
            flush()[0].message,
            Message::Credit {
                asset: SpokeAsset::Stable,
                to: wallet(),
                amount: 200 * UNIT,
                order: None,
                refund: false,
                rate: 0,
            }
        );
        System::assert_has_event(
            Event::BridgedOut {
                who: ALICE,
                bridge: BASE,
                source: NativeAsset::Tusd,
                amount: 200 * UNIT,
                destination: Destination::Stable { to: wallet() },
                amount_out: 200 * UNIT,
                fee: TaoBalance::from(0),
            }
            .into(),
        );
        // A bridge releases only what it brought in.
        assert_noop!(
            Rails::bridge_out(
                RuntimeOrigin::signed(ALICE),
                ETHEREUM,
                NativeAsset::Tusd,
                100 * UNIT,
                Destination::Stable { to: wallet() },
                0
            ),
            pallet_tusd::Error::<Test>::RedeemExceedsOutstanding
        );
        assert_invariants();
    });
}

#[test]
fn an_account_wraps_tao_and_buys_alpha_tokens() {
    new_test_ext().execute_with(|| {
        open_market(CHUTES, "Chutes");
        fund_tao(ALICE, 101 * UNIT);

        assert_ok!(Rails::bridge_out(
            RuntimeOrigin::signed(ALICE),
            BASE,
            NativeAsset::Tao,
            40 * UNIT,
            Destination::Tao { to: wallet() },
            40 * UNIT
        ));
        assert_eq!(TaoSupply::<Test>::get(BASE), TaoBalance::from(40 * UNIT));
        assert_eq!(
            Balances::free_balance(Rails::reserve_account()),
            TaoBalance::from(40 * UNIT)
        );

        assert_ok!(Rails::bridge_out(
            RuntimeOrigin::signed(ALICE),
            BASE,
            NativeAsset::Tao,
            10 * UNIT,
            Destination::Alpha {
                netuid: netuid(CHUTES),
                to: wallet()
            },
            0
        ));
        assert_eq!(
            AlphaSupply::<Test>::get(netuid(CHUTES), BASE),
            10 * UNIT * ALPHA_PER_TAO
        );
        assert_eq!(Balances::free_balance(ALICE), TaoBalance::from(51 * UNIT));
        assert_eq!(flush().len(), 2);
        assert_invariants();
    });
}

#[test]
fn a_payment_from_bittensor_fails_as_a_whole() {
    new_test_ext().execute_with(|| {
        fund_tao(ALICE, 101 * UNIT);
        let pay = |destination: Destination<AccountId>, min_out: u64| {
            Rails::bridge_out(
                RuntimeOrigin::signed(ALICE),
                BASE,
                NativeAsset::Tao,
                10 * UNIT,
                destination,
                min_out,
            )
        };
        // Less than the minimum: nothing moved, nothing queued.
        assert_noop!(
            pay(Destination::Stable { to: wallet() }, 4_000 * UNIT),
            Error::<Test>::SlippageExceeded
        );
        // What an account does directly on Bittensor is not a bridge payment.
        for destination in [
            Destination::NativeTao { to: BOB },
            Destination::NativeTusd { to: BOB },
            Destination::Credits {
                netuid: netuid(LIUM),
                service: SERVICE,
                beneficiary: wallet(),
            },
        ] {
            assert_noop!(pay(destination, 0), Error::<Test>::InvalidRoute);
        }
        // A bridge that is disabled, and an account without the funds.
        assert_noop!(
            Rails::bridge_out(
                RuntimeOrigin::signed(ALICE),
                7,
                NativeAsset::Tao,
                UNIT,
                Destination::Tao { to: wallet() },
                0
            ),
            Error::<Test>::BridgeDisabled
        );
        assert!(
            Rails::bridge_out(
                RuntimeOrigin::signed(BOB),
                BASE,
                NativeAsset::Tao,
                UNIT,
                Destination::Tao { to: wallet() },
                0
            )
            .is_err()
        );
        assert_invariants();
    });
}

// ---------------------------------------------------------------- a subnet that ends

#[test]
fn a_dissolved_subnets_tokens_redeem_the_tao_its_stake_was_paid_out_in() {
    new_test_ext().execute_with(|| {
        open_market(CHUTES, "Chutes");
        let alpha = Destination::Alpha {
            netuid: netuid(CHUTES),
            to: wallet(),
        };
        let shares = settled(SpokeAsset::Stable, 4_000 * UNIT, alpha.clone());
        flush();
        let vault = Rails::vault_account(netuid(CHUTES));

        // The subnet is being dissolved: its stake is not paid out yet.
        set_subnet_status(CHUTES, SubnetStatus::Dissolving);
        for (source, destination) in [
            (SpokeAsset::Stable, alpha.clone()),
            (
                SpokeAsset::Alpha(netuid(CHUTES)),
                Destination::NativeTao { to: ALICE },
            ),
        ] {
            assert_eq!(
                refunded(source, shares / 4, destination),
                Error::<Test>::MarketSettling.into()
            );
        }
        assert_eq!(Rails::alpha_rate(netuid(CHUTES)), None);

        // Dissolution pays the vault's stake to the vault account as 8 TAO.
        set_subnet_status(CHUTES, SubnetStatus::Absent);
        fund_tao(vault.clone(), 8 * UNIT);
        flush();

        // The token can no longer be bought.
        assert_eq!(
            refunded(SpokeAsset::Stable, 4_000 * UNIT, alpha.clone()),
            Error::<Test>::MarketClosed.into()
        );
        // A quarter of the shares redeem a quarter of what the vault account holds, whoever
        // comes first.
        let pot = 8 * UNIT + EXISTENTIAL_DEPOSIT;
        let first = settled(
            SpokeAsset::Alpha(netuid(CHUTES)),
            shares / 4,
            Destination::NativeTao { to: ALICE },
        );
        assert_eq!(first, pot / 4);
        let rest = settled(
            SpokeAsset::Alpha(netuid(CHUTES)),
            shares - shares / 4,
            Destination::NativeTao { to: BOB },
        );
        assert_eq!(rest, pot - pot / 4);
        assert_eq!(Balances::free_balance(vault), TaoBalance::from(0));
        assert_eq!(Markets::<Test>::get(netuid(CHUTES)).unwrap().shares, 0);

        // The netuid is registered again: another subnet, not this market's.
        set_subnet_status(CHUTES, SubnetStatus::Live { generation: 2 });
        assert_eq!(
            refunded(SpokeAsset::Stable, 4_000 * UNIT, alpha),
            Error::<Test>::MarketClosed.into()
        );
        assert_noop!(
            Rails::open_market(RuntimeOrigin::root(), netuid(CHUTES), UNIT.into()),
            Error::<Test>::MarketExists
        );
        assert_invariants();
    });
}

// ---------------------------------------------------------------- quotes

#[test]
fn a_quote_is_what_the_order_then_delivers_and_changes_nothing() {
    new_test_ext().execute_with(|| {
        open_market(CHUTES, "Chutes");
        add_subnet(LIUM, "lium.io");
        add_service(LIUM, SERVICE);
        let alpha = Destination::Alpha {
            netuid: netuid(CHUTES),
            to: wallet(),
        };
        for destination in [
            Destination::NativeTusd { to: ALICE },
            Destination::NativeTao { to: ALICE },
            Destination::Tao { to: wallet() },
            alpha.clone(),
            credits(SERVICE),
        ] {
            let root = sp_io::storage::root(sp_runtime::StateVersion::V1);
            let quote =
                Rails::quote_order(BASE, SpokeAsset::Stable, 400 * UNIT, destination.clone());
            assert_eq!(
                sp_io::storage::root(sp_runtime::StateVersion::V1),
                root,
                "a quote changed state"
            );
            assert_eq!(
                quote,
                Ok(settled(SpokeAsset::Stable, 400 * UNIT, destination))
            );
        }
        flush();
        // A sale, and orders that would be refunded or dropped.
        let shares = AlphaSupply::<Test>::get(netuid(CHUTES), BASE);
        let quote = Rails::quote_order(
            BASE,
            SpokeAsset::Alpha(netuid(CHUTES)),
            shares,
            Destination::Stable { to: wallet() },
        );
        assert_eq!(
            quote,
            Ok(settled(
                SpokeAsset::Alpha(netuid(CHUTES)),
                shares,
                Destination::Stable { to: wallet() }
            ))
        );
        assert_eq!(
            Rails::quote_order(BASE, SpokeAsset::Stable, UNIT, credits(STRANGER)),
            Err(NO_SERVICE)
        );
        assert_eq!(
            Rails::quote_order(
                BASE,
                SpokeAsset::Tao,
                UNIT,
                Destination::Stable { to: wallet() }
            ),
            Err(Error::<Test>::Unbacked.into())
        );
        assert_eq!(
            Rails::quote_order(
                7,
                SpokeAsset::Stable,
                UNIT,
                Destination::NativeTusd { to: ALICE }
            ),
            Err(Error::<Test>::BridgeDisabled.into())
        );
    });
}

#[test]
fn a_payment_from_bittensor_is_quoted_as_it_then_settles() {
    new_test_ext().execute_with(|| {
        open_market(CHUTES, "Chutes");
        fund_tao(ALICE, 101 * UNIT);
        // The bridge releases only the dollars it brought in.
        settled(
            SpokeAsset::Stable,
            10_000 * UNIT,
            Destination::NativeTusd { to: ALICE },
        );
        for (source, amount, destination) in [
            (
                NativeAsset::Tusd,
                200 * UNIT,
                Destination::Stable { to: wallet() },
            ),
            (
                NativeAsset::Tao,
                10 * UNIT,
                Destination::Stable { to: wallet() },
            ),
            (
                NativeAsset::Tao,
                10 * UNIT,
                Destination::Tao { to: wallet() },
            ),
            (
                NativeAsset::Tusd,
                200 * UNIT,
                Destination::Alpha {
                    netuid: netuid(CHUTES),
                    to: wallet(),
                },
            ),
        ] {
            let root = sp_io::storage::root(sp_runtime::StateVersion::V1);
            let quote =
                Rails::quote_bridge_out(&ALICE, BASE, source, amount, destination.clone()).unwrap();
            assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), root);
            // With the quote as the minimum the payment goes through; with one more it fails.
            assert_noop!(
                Rails::bridge_out(
                    RuntimeOrigin::signed(ALICE),
                    BASE,
                    source,
                    amount,
                    destination.clone(),
                    quote + 1
                ),
                Error::<Test>::SlippageExceeded
            );
            assert_ok!(Rails::bridge_out(
                RuntimeOrigin::signed(ALICE),
                BASE,
                source,
                amount,
                destination,
                quote
            ));
        }
        // What would fail is quoted as its error.
        assert_eq!(
            Rails::quote_bridge_out(
                &BOB,
                BASE,
                NativeAsset::Tusd,
                UNIT,
                Destination::Stable { to: wallet() }
            ),
            Err(pallet_tusd::Error::<Test>::InsufficientBalance.into())
        );
        assert_ok!(Rails::set_paused(RuntimeOrigin::signed(GUARDIAN), true));
        assert_eq!(
            Rails::quote_bridge_out(
                &ALICE,
                BASE,
                NativeAsset::Tao,
                UNIT,
                Destination::Tao { to: wallet() }
            ),
            Err(Error::<Test>::Paused.into())
        );
        assert_eq!(
            Rails::quote_order(
                BASE,
                SpokeAsset::Stable,
                UNIT,
                Destination::NativeTusd { to: ALICE }
            ),
            Err(Error::<Test>::Paused.into())
        );
    });
}

// ---------------------------------------------------------------- vault arithmetic

#[test]
fn vault_arithmetic_rounds_against_the_trader() {
    // An empty vault: one share for one alpha.
    assert_eq!(shares_for_alpha(5 * UNIT, 0, 0), Some(5 * UNIT));
    assert_eq!(
        alpha_for_shares(5 * UNIT, 5 * UNIT, 5 * UNIT),
        Some(5 * UNIT)
    );
    assert_eq!(rate(0, 0), RATE_SCALE);
    assert_eq!(rate(5 * UNIT, 5 * UNIT), RATE_SCALE);
    assert_eq!(rate(3 * UNIT + 2 * VIRTUAL as u64, UNIT), 3 * RATE_SCALE);

    for (staked, shares) in [
        (0, 0),
        (1, 1),
        (7 * UNIT + 3, 5 * UNIT + 1),
        (21_000_000 * UNIT, 13_000_000 * UNIT),
        (u64::MAX, u64::MAX / 3),
        (u64::MAX / 7, u64::MAX),
    ] {
        for amount in [1, 999, UNIT, 123_456_789_012, u64::MAX / 2] {
            // Buying and selling back at once never returns more than was paid.
            if let Some(minted) = shares_for_alpha(amount, staked, shares)
                && let (Some(staked_after), Some(shares_after)) =
                    (staked.checked_add(amount), shares.checked_add(minted))
            {
                let back = alpha_for_shares(minted, staked_after, shares_after).unwrap();
                assert!(back <= amount, "{staked} {shares} {amount}: {back}");
                // And the rate does not fall for those who stay.
                assert!(rate(staked_after, shares_after) >= rate(staked, shares));
            }
            // Redeeming never takes more than a share's worth, so the rate cannot fall.
            if amount <= shares {
                let paid = alpha_for_shares(amount, staked, shares).unwrap();
                assert!(paid <= staked || shares == 0);
                assert!(rate(staked.saturating_sub(paid), shares - amount) >= rate(staked, shares));
            }
        }
    }
}

#[test]
fn a_gift_to_the_vault_cannot_round_the_next_buyer_away() {
    // The classic attack on a share vault: buy the smallest share, give the vault a
    // fortune, and let the next buyer's shares round down to nothing.
    let attacker_alpha = 1u64;
    let attacker_shares = shares_for_alpha(attacker_alpha, 0, 0).unwrap();
    assert_eq!(attacker_shares, 1);
    let gift = 1_000_000 * UNIT;
    let staked = attacker_alpha + gift;

    // The victim buys with a thousand alpha.
    let victim_alpha = 1_000 * UNIT;
    let victim_shares = shares_for_alpha(victim_alpha, staked, attacker_shares).unwrap();
    assert!(victim_shares > 0, "the victim got nothing");
    let staked = staked + victim_alpha;
    let shares = attacker_shares + victim_shares;

    // The victim's shares are worth what she paid, to a millionth.
    let victim_back = alpha_for_shares(victim_shares, staked, shares).unwrap();
    assert!(victim_back >= victim_alpha - victim_alpha / 1_000_000);
    // The attacker's share recovers next to nothing of the gift: it went to the virtual share.
    let attacker_back = alpha_for_shares(attacker_shares, staked, shares).unwrap();
    assert!(attacker_back < gift / 100_000, "{attacker_back}");
}

// ---------------------------------------------------------------- everything at once

/// Random orders of every kind, payments from Bittensor, emissions and gateway flushes.
/// Whatever is settled, refunded or rejected: the hub account ends empty, the reserve
/// covers canonical TAO, every vault covers its shares, and no nonce is reused.
#[test]
fn hub_invariants_hold_under_random_operations() {
    new_test_ext().execute_with(|| {
        open_market(CHUTES, "Chutes");
        open_market(LIUM, "lium.io");
        add_service(LIUM, SERVICE);
        fund_tao(ALICE, 1_000 * UNIT);
        fund_tusd(ALICE, 400_000 * UNIT);
        let mut state = 0xD1B5_4A32_D192_ED03u64;
        let mut next = move |bound: u64| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state % bound.max(1)
        };
        let mut outcomes = [0u32; 3];
        let first_message = NextMessage::<Test>::get(BASE);
        let mut message_nonces = Vec::new();
        for step in 0..2_500u64 {
            System::set_block_number(100 + step / 2);
            let market = if next(2) == 0 { CHUTES } else { LIUM };
            let to = H160::repeat_byte(next(4) as u8);
            let destination = match next(7) {
                0 => Destination::Stable { to },
                1 => Destination::Tao { to },
                2 => Destination::Alpha {
                    netuid: netuid(market),
                    to,
                },
                3 => Destination::Credits {
                    netuid: netuid(LIUM),
                    service: SERVICE,
                    beneficiary: to,
                },
                4 => Destination::NativeTusd { to: BOB },
                5 => Destination::NativeTao { to: BOB },
                _ => Destination::Alpha {
                    netuid: netuid(LIUM),
                    to,
                },
            };
            match next(10) {
                // An order from Base.
                0..=5 => {
                    // Now and then the portal claims a burn beyond its supply.
                    let beyond = if next(6) == 0 { 1 + next(UNIT) } else { 0 };
                    let pick = next(u64::MAX);
                    let burn = |supply: u64| match beyond {
                        0 => pick % (supply + 1),
                        _ => supply + beyond,
                    };
                    let (source, amount) = match next(3) {
                        0 => (SpokeAsset::Stable, next(3_000 * UNIT)),
                        1 => (SpokeAsset::Tao, burn(TaoSupply::<Test>::get(BASE).into())),
                        _ => (
                            SpokeAsset::Alpha(netuid(market)),
                            burn(AlphaSupply::<Test>::get(netuid(market), BASE)),
                        ),
                    };
                    let mut order = order(source, amount, destination);
                    order.min_out = next(3) * next(40 * UNIT);
                    let nonce = order.nonce;
                    match execute(order) {
                        Ok(()) => match Receipts::<Test>::get(BASE, nonce).unwrap().outcome {
                            Outcome::Settled { .. } => outcomes[0] += 1,
                            Outcome::Refunded { .. } => outcomes[1] += 1,
                            _ => outcomes[2] += 1,
                        },
                        Err(error) => assert_eq!(error, Error::<Test>::OutboxFull.into()),
                    }
                }
                // A payment from Bittensor.
                6 | 7 => {
                    let source = if next(2) == 0 {
                        NativeAsset::Tao
                    } else {
                        NativeAsset::Tusd
                    };
                    let _ = Rails::bridge_out(
                        RuntimeOrigin::signed(ALICE),
                        BASE,
                        source,
                        next(20 * UNIT),
                        destination,
                        0,
                    );
                }
                // Emissions.
                8 => accrue(market, next(5 * UNIT)),
                // The gateway hands messages to the transport.
                _ => message_nonces.extend(flush().into_iter().map(|outbound| outbound.nonce)),
            }
            assert_invariants();
        }
        message_nonces.extend(flush().into_iter().map(|outbound| outbound.nonce));
        assert!(outcomes.iter().all(|count| *count > 50), "{outcomes:?}");
        // Every message nonce of the bridge went out exactly once, in order.
        assert_eq!(
            message_nonces,
            (first_message..NextMessage::<Test>::get(BASE)).collect::<Vec<_>>()
        );
    });
}
