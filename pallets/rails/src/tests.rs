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
    AlphaSupply, Destination, Error, Event, Markets, Message, NativeAsset, NextMessage, NextOrder,
    Order, Outbound, Outbox, OutboxBounds, Outcome, Paused, RailsStaking, Receipts, SpokeAsset,
    SubnetStatus, TaoCap, TaoSupply, TokenInfo, Tokens,
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

/// The messages waiting for the gateway, oldest first.
fn outbox() -> Vec<Outbound> {
    let (head, tail) = OutboxBounds::<Test>::get();
    (head..tail)
        .map(|position| Outbox::<Test>::get(position).unwrap())
        .collect()
}

/// The gateway hands every waiting message to the transport.
fn flush() -> Vec<Outbound> {
    let mut sent = Vec::new();
    while let Some((position, domain, sender, outbound)) = Rails::next_outbound() {
        assert_eq!((domain, sender), (outbound.bridge, portal(outbound.bridge)));
        assert_ok!(Rails::ack_outbound(
            RuntimeOrigin::signed(GATEWAY),
            position
        ));
        sent.push(outbound);
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
        assert_noop!(Rails::ack_outbound(signed(), 0), Error::<Test>::NotGateway);
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

        assert_eq!(paid, quoted);
        // Two fees of 0.3% and the price moved by her own trades.
        assert!(paid > 3_970 * UNIT && paid < 3_980 * UNIT);
        assert_eq!(TaoSupply::<Test>::get(BASE), TaoBalance::from(0));
        assert_eq!(
            Balances::free_balance(Rails::reserve_account()),
            TaoBalance::from(0)
        );
        // The bridge released what it pays out; the rest of her dollars stayed in the pool.
        assert_eq!(outstanding(BASE), 4_000 * UNIT - paid);
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
fn the_gateway_takes_messages_first_in_first_out() {
    new_test_ext().execute_with(|| {
        assert_eq!(Rails::next_outbound(), None);
        for _ in 0..3 {
            settled(
                SpokeAsset::Stable,
                400 * UNIT,
                Destination::Tao { to: wallet() },
            );
        }
        assert_eq!(OutboxBounds::<Test>::get(), (0, 3));
        assert_eq!(NextMessage::<Test>::get(BASE), 3);

        let (position, domain, sender, outbound) = Rails::next_outbound().unwrap();
        assert_eq!(
            (position, domain, sender, outbound.nonce),
            (0, BASE, portal(BASE), 0)
        );
        // Only the head leaves, and only through the gateway.
        assert_noop!(
            Rails::ack_outbound(RuntimeOrigin::signed(GATEWAY), 1),
            Error::<Test>::NotOutboxHead
        );
        assert_noop!(
            Rails::ack_outbound(RuntimeOrigin::signed(ALICE), 0),
            Error::<Test>::NotGateway
        );
        assert_ok!(Rails::ack_outbound(RuntimeOrigin::signed(GATEWAY), 0));
        System::assert_last_event(Event::MessageDispatched { position: 0 }.into());
        assert_noop!(
            Rails::ack_outbound(RuntimeOrigin::signed(GATEWAY), 0),
            Error::<Test>::NotOutboxHead
        );

        // A paused hub shows and hands over nothing.
        assert_ok!(Rails::set_paused(RuntimeOrigin::signed(GUARDIAN), true));
        assert_eq!(Rails::next_outbound(), None);
        assert_noop!(
            Rails::ack_outbound(RuntimeOrigin::signed(GATEWAY), 1),
            Error::<Test>::Paused
        );
        assert_ok!(Rails::set_paused(RuntimeOrigin::root(), false));

        let sent: Vec<u64> = flush().into_iter().map(|outbound| outbound.nonce).collect();
        assert_eq!(sent, vec![1, 2]);
        assert_eq!(Rails::next_outbound(), None);
        assert_noop!(
            Rails::ack_outbound(RuntimeOrigin::signed(GATEWAY), 3),
            Error::<Test>::NotOutboxHead
        );
    });
}

#[test]
fn an_order_waits_while_the_outbox_is_full() {
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
        // An order that needs no answer still goes through.
        settled(
            SpokeAsset::Stable,
            40 * UNIT,
            Destination::NativeTusd { to: ALICE },
        );

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
