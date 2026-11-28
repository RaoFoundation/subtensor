//! The Rails precompile: the door between the hub of the canonical rails and the gateway
//! contract on Bittensor's EVM.
//!
//! The gateway receives a portal's order from the transport and hands it to `execute`,
//! which dispatches `Rails::execute` signed by the caller. The hub takes the call only from
//! the gateway's account, so `execute` is safe to expose: any other caller is refused.
//!
//! In transactions of its own, the gateway reads the hub's messages for a bridge with
//! `nextOutbound`, and for each one tells the hub what the transport charges
//! (`ackOutbound`), pays the transport and hands it the message. The hub moves that charge
//! from the account of the bridge to the gateway, and refuses a charge above what it pays
//! for the message. The gateway defers a message that is not paid for (`deferOutbound`),
//! and sends it later (`outbound`, `ackOutbound`).
//!
//! The views let a client quote an order, follow it, and read the rate of an alpha token
//! and the credits of an address through an Ethereum RPC. The precompile emits no logs:
//! the gateway contract does.

use core::marker::PhantomData;

use fp_evm::{ExitError, PrecompileFailure};
use frame_support::{
    dispatch::{DispatchInfo, GetDispatchInfo, PostDispatchInfo},
    traits::IsSubType,
};
use frame_system::RawOrigin;
use pallet_evm::{
    AddressMapping, BalanceConverter, EvmBalance, GasWeightMapping, PrecompileHandle,
    SubstrateBalance,
};
use pallet_rails::{Outcome, RailsStaking, WeightInfo};
use precompile_utils::{
    EvmResult,
    prelude::{Address, BoundedBytes, UnboundedBytes, UnboundedString},
};
use sp_core::{ByteArray, ConstU32, H256, U256};
use sp_runtime::traits::{AsSystemOriginSigner, Dispatchable};
use subtensor_runtime_common::{NetUid, TaoBalance, rails::Payer};

use crate::{PrecompileExt, PrecompileHandleExt, rails_wire};

/// What became of an order, as `receipt` and `execute` report it.
const STATUS_NONE: u8 = 0;
const STATUS_SETTLED: u8 = 1;
const STATUS_REFUNDED: u8 = 2;
const STATUS_REJECTED: u8 = 3;
const STATUS_SKIPPED: u8 = 4;

type Order = BoundedBytes<ConstU32<{ rails_wire::ORDER_LEN as u32 }>>;

pub struct RailsPrecompile<R>(PhantomData<R>);

impl<R> PrecompileExt<R::AccountId> for RailsPrecompile<R>
where
    R: frame_system::Config
        + pallet_balances::Config
        + pallet_evm::Config
        + pallet_rails::Config
        + pallet_subtensor::Config
        + pallet_shield::Config
        + pallet_subtensor_proxy::Config
        + Send
        + Sync
        + scale_info::TypeInfo,
    R::AccountId: From<[u8; 32]> + ByteArray,
    <R as frame_system::Config>::RuntimeOrigin: AsSystemOriginSigner<R::AccountId> + Clone,
    <R as frame_system::Config>::RuntimeCall: From<pallet_rails::Call<R>>
        + GetDispatchInfo
        + Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>
        + IsSubType<pallet_balances::Call<R>>
        + IsSubType<pallet_subtensor::Call<R>>
        + IsSubType<pallet_shield::Call<R>>
        + IsSubType<pallet_subtensor_proxy::Call<R>>,
    <R as pallet_evm::Config>::AddressMapping: AddressMapping<R::AccountId>,
{
    const INDEX: u64 = 2069;
}

#[precompile_utils::precompile]
impl<R> RailsPrecompile<R>
where
    R: frame_system::Config
        + pallet_balances::Config
        + pallet_evm::Config
        + pallet_rails::Config
        + pallet_subtensor::Config
        + pallet_shield::Config
        + pallet_subtensor_proxy::Config
        + Send
        + Sync
        + scale_info::TypeInfo,
    R::AccountId: From<[u8; 32]> + ByteArray,
    <R as frame_system::Config>::RuntimeOrigin: AsSystemOriginSigner<R::AccountId> + Clone,
    <R as frame_system::Config>::RuntimeCall: From<pallet_rails::Call<R>>
        + GetDispatchInfo
        + Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>
        + IsSubType<pallet_balances::Call<R>>
        + IsSubType<pallet_subtensor::Call<R>>
        + IsSubType<pallet_shield::Call<R>>
        + IsSubType<pallet_subtensor_proxy::Call<R>>,
    <R as pallet_evm::Config>::AddressMapping: AddressMapping<R::AccountId>,
{
    /// Settles the order that the portal `sender` on transport domain `domain` sent.
    /// Signed by the caller: the hub takes it only from the gateway.
    ///
    /// Returns what became of the order, what it delivered, and the bridge of its portal:
    /// the queue that holds the hub's answer.
    #[precompile::public("execute(uint32,bytes32,bytes)")]
    fn execute(
        handle: &mut impl PrecompileHandle,
        domain: u32,
        sender: H256,
        order: Order,
    ) -> EvmResult<(u8, u64, u32)> {
        let gateway = handle.caller_account_id::<R>()?;
        let order = decode_order::<R>(&order)?;
        let nonce = order.nonce;
        let call = pallet_rails::Call::<R>::execute {
            domain,
            sender,
            order,
        };
        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(gateway))?;

        // The hub knew the portal, or the dispatch would have failed.
        handle.record_db_reads::<R>(2)?;
        let bridge = pallet_tusd::Pallet::<R>::bridge_of(domain, sender).unwrap_or_default();
        let (status, amount_out, _) = receipt_of::<R>(bridge, nonce);
        Ok((status, amount_out, bridge))
    }

    /// Removes message `nonce` of `bridge`, the head of the hub's queue or a deferred
    /// message, and moves `paid` from the bridge's account on the hub to the caller:
    /// what the caller pays the transport for the message from the hub's TAO, in the EVM's
    /// unit, with 18 decimals.
    /// The hub counts it in its own unit, rounded up. Signed by the caller: the hub takes
    /// it only from the gateway, which calls this in the transaction that pays the
    /// transport and hands it the message. Fails if `paid` is more than the hub pays for
    /// the message: the limit that `nextOutbound` returns.
    #[precompile::public("ackOutbound(uint32,uint64,uint256)")]
    fn ack_outbound(
        handle: &mut impl PrecompileHandle,
        bridge: u32,
        nonce: u64,
        paid: U256,
    ) -> EvmResult<()> {
        let gateway = handle.caller_account_id::<R>()?;
        let call = pallet_rails::Call::<R>::ack_outbound {
            bridge,
            nonce,
            paid: tao_rounded_up::<R>(paid)?,
        };
        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(gateway))
    }

    /// Defers message `nonce`, the head of the hub's queue for `bridge`: takes it out of
    /// the line, so that the messages behind it can leave. The message stays in the hub.
    /// Signed by the caller: the hub takes it only from the gateway, which calls it for a
    /// message that is not paid for.
    #[precompile::public("deferOutbound(uint32,uint64)")]
    fn defer_outbound(
        handle: &mut impl PrecompileHandle,
        bridge: u32,
        nonce: u64,
    ) -> EvmResult<()> {
        let gateway = handle.caller_account_id::<R>()?;
        let call = pallet_rails::Call::<R>::defer_outbound { bridge, nonce };
        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(gateway))
    }

    /// Message `nonce` of `bridge`, if the gateway can take it now: whether it can, the
    /// transport domain and portal the message goes to, its body as the portal reads it,
    /// and the most the hub pays the transport for it now, in the EVM's unit. The gateway
    /// can take the head of the bridge's queue and the deferred messages.
    #[precompile::public("outbound(uint32,uint64)")]
    #[precompile::view]
    fn outbound(
        handle: &mut impl PrecompileHandle,
        bridge: u32,
        nonce: u64,
    ) -> EvmResult<(bool, u32, H256, UnboundedBytes, U256)> {
        // The message and its route: 4 reads. The hub's limit: 6.
        handle.record_db_reads::<R>(10)?;
        Ok(match pallet_rails::Pallet::<R>::outbound(bridge, nonce) {
            Some((domain, portal, outbound)) => (
                true,
                domain,
                portal,
                rails_wire::encode_message(&outbound).into(),
                limit_of::<R>(bridge)?,
            ),
            None => (
                false,
                0,
                H256::zero(),
                UnboundedBytes::from(&[][..]),
                U256::zero(),
            ),
        })
    }

    /// The message at the head of the hub's queue for `bridge`: whether there is one, its
    /// nonce, the transport domain and portal it goes to, its body as the portal reads
    /// it, and the most the hub pays the transport for it now, in the EVM's unit.
    #[precompile::public("nextOutbound(uint32)")]
    #[precompile::view]
    fn next_outbound(
        handle: &mut impl PrecompileHandle,
        bridge: u32,
    ) -> EvmResult<(bool, u64, u32, H256, UnboundedBytes, U256)> {
        // The head of the queue: 2 reads. Then as `outbound`.
        handle.record_db_reads::<R>(12)?;
        Ok(match pallet_rails::Pallet::<R>::next_outbound(bridge) {
            Some((domain, portal, outbound)) => (
                true,
                outbound.nonce,
                domain,
                portal,
                rails_wire::encode_message(&outbound).into(),
                limit_of::<R>(bridge)?,
            ),
            None => (
                false,
                0,
                0,
                H256::zero(),
                UnboundedBytes::from(&[][..]),
                U256::zero(),
            ),
        })
    }

    /// What `order`, sent now from the spoke of `bridge`, would deliver: whether the hub
    /// would settle it, the amount of its destination, and otherwise the reason. The
    /// order's nonce, payer, minimum and deadline are not read.
    ///
    /// Runs the order and undoes it, so it costs the gas of settling one.
    #[precompile::public("quote(uint32,bytes)")]
    #[precompile::view]
    fn quote(
        handle: &mut impl PrecompileHandle,
        bridge: u32,
        order: Order,
    ) -> EvmResult<(bool, u64, UnboundedString)> {
        let weight = <R as pallet_rails::Config>::WeightInfo::execute()
            .saturating_add(<R as pallet_rails::Config>::Staking::weight());
        handle.record_external_cost(Some(weight.ref_time()), Some(weight.proof_size()), None)?;
        handle.record_cost(<R as pallet_evm::Config>::GasWeightMapping::weight_to_gas(
            weight,
        ))?;

        let order = decode_order::<R>(&order)?;
        Ok(
            match pallet_rails::Pallet::<R>::quote_order(
                bridge,
                order.source,
                order.amount,
                order.destination,
            ) {
                Ok(amount_out) => (true, amount_out, UnboundedString::from("")),
                Err(error) => {
                    let reason: &'static str = error.into();
                    (false, 0, UnboundedString::from(reason))
                }
            },
        )
    }

    /// What became of order `nonce` of `bridge`: 0 unknown (not taken yet, or its receipt
    /// is no longer kept), 1 settled, 2 refunded, 3 rejected, 4 skipped. Then what it
    /// delivered, and the block it was disposed of in.
    #[precompile::public("receipt(uint32,uint64)")]
    #[precompile::view]
    fn receipt(
        handle: &mut impl PrecompileHandle,
        bridge: u32,
        nonce: u64,
    ) -> EvmResult<(u8, u64, u32)> {
        handle.record_db_reads::<R>(1)?;
        Ok(receipt_of::<R>(bridge, nonce))
    }

    /// The nonce of the next order the hub takes from `bridge`: every order below it has
    /// been disposed of.
    #[precompile::public("nextOrder(uint32)")]
    #[precompile::view]
    fn next_order(handle: &mut impl PrecompileHandle, bridge: u32) -> EvmResult<u64> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_rails::NextOrder::<R>::get(bridge))
    }

    /// The alpha that one share of the vault of `netuid`, which is one unit of the
    /// subnet's alpha token, is worth now, times 1e18. Zero if the subnet has no market or
    /// its market only winds down.
    #[precompile::public("alphaRate(uint16)")]
    #[precompile::view]
    fn alpha_rate(handle: &mut impl PrecompileHandle, netuid: u16) -> EvmResult<U256> {
        handle.record_db_reads::<R>(12)?;
        Ok(pallet_rails::Pallet::<R>::alpha_rate(NetUid::from(netuid))
            .unwrap_or_default()
            .into())
    }

    /// The gamma, with 9 decimals, that payments through the rails have spent at `service`
    /// on `netuid` for the address `payer`, since ever. A service credits an address by
    /// the growth of this total.
    #[precompile::public("creditsOf(uint16,bytes32,address)")]
    #[precompile::view]
    fn credits_of(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        service: H256,
        payer: Address,
    ) -> EvmResult<u64> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::GammaSpentByPayer::<R>::get((
            NetUid::from(netuid),
            R::AccountId::from(service.0),
            Payer::Evm(payer.0),
        ))
        .into())
    }
}

fn decode_order<R>(body: &Order) -> EvmResult<pallet_rails::Order<R::AccountId>>
where
    R: frame_system::Config,
    R::AccountId: From<[u8; 32]>,
{
    rails_wire::decode_order(body.as_bytes()).ok_or(PrecompileFailure::Error {
        exit_status: ExitError::Other("not an order".into()),
    })
}

/// The most the hub pays for one message of `bridge` now, in the EVM's unit of TAO.
fn limit_of<R: pallet_evm::Config + pallet_rails::Config>(bridge: u32) -> EvmResult<U256> {
    let limit = pallet_rails::Pallet::<R>::message_limit(bridge);
    <R as pallet_evm::Config>::BalanceConverter::into_evm_balance(SubstrateBalance::from(
        u64::from(limit),
    ))
    .map(EvmBalance::into_u256)
    .ok_or(PrecompileFailure::Error {
        exit_status: ExitError::Other("the limit does not fit the EVM's unit".into()),
    })
}

/// An amount in the EVM's unit of TAO, in the hub's unit, rounded up: the hub never counts
/// less than the gateway paid.
fn tao_rounded_up<R: pallet_evm::Config>(amount: U256) -> EvmResult<TaoBalance> {
    let whole = <R as pallet_evm::Config>::BalanceConverter::into_substrate_balance(
        EvmBalance::new(amount),
    )
    .ok_or(PrecompileFailure::Error {
        exit_status: ExitError::Other("the amount paid is too large".into()),
    })?;
    let exact = <R as pallet_evm::Config>::BalanceConverter::into_evm_balance(whole)
        .is_some_and(|back| back.into_u256() == amount);
    Ok(whole
        .into_u64_saturating()
        .saturating_add(u64::from(!exact))
        .into())
}

/// The status, amount delivered and block of an order's receipt.
fn receipt_of<R: pallet_rails::Config>(bridge: u32, nonce: u64) -> (u8, u64, u32) {
    match pallet_rails::Receipts::<R>::get(bridge, nonce) {
        None => (STATUS_NONE, 0, 0),
        Some(receipt) => {
            let (status, amount_out) = match receipt.outcome {
                Outcome::Settled { amount_out } => (STATUS_SETTLED, amount_out),
                Outcome::Refunded { .. } => (STATUS_REFUNDED, 0),
                Outcome::Rejected { .. } => (STATUS_REJECTED, 0),
                Outcome::Skipped => (STATUS_SKIPPED, 0),
            };
            (status, amount_out, receipt.block)
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::arithmetic_side_effects,
        clippy::indexing_slicing,
        clippy::unwrap_used
    )]

    use super::*;
    use crate::mock::{
        AccountId, Balances, CanonicalPool, Rails, Runtime, RuntimeOrigin, SubtensorModule, System,
        Timestamp, Tusd, addr_from_index, execute_precompile, fund_account, mapped_account,
        new_test_ext, precompiles, selector_u32,
    };
    use frame_support::{BoundedVec, assert_ok};
    use pallet_rails::{Destination, Message, Outbound, SpokeAsset};
    use precompile_utils::{solidity::encode_with_selector, testing::PrecompileTesterExt};
    use sp_core::H160;
    use sp_runtime::Permill;
    use subtensor_runtime_common::{
        AlphaBalance, TaoBalance, TusdBalance,
        rails::{OriginChain, StableAsset},
    };
    use subtensor_swap_interface::SwapHandler;

    const UNIT: u64 = 1_000_000_000;
    /// The most the hub pays the transport for one message to Base.
    const RESERVE: u64 = UNIT / 1_000;
    /// The bridge to Base, and its transport domain.
    const BASE: u32 = 8453;
    const NETUID: u16 = 1;
    /// 400 dollars per TAO.
    const PRICE: u64 = 400 * UNIT;

    fn portal() -> H256 {
        H256::repeat_byte(0xB0)
    }

    fn gateway() -> H160 {
        addr_from_index(0x6A7E)
    }

    fn stranger() -> H160 {
        addr_from_index(0xBAD)
    }

    fn wallet() -> H160 {
        H160::repeat_byte(0xBB)
    }

    fn account(seed: u8) -> AccountId {
        AccountId::from([seed; 32])
    }

    fn rails() -> H160 {
        addr_from_index(RailsPrecompile::<Runtime>::INDEX)
    }

    fn order(
        nonce: u64,
        source: SpokeAsset,
        amount: u64,
        destination: Destination<AccountId>,
    ) -> pallet_rails::Order<AccountId> {
        pallet_rails::Order {
            nonce,
            payer: H160::repeat_byte(0xAA),
            source,
            amount,
            destination,
            min_out: 0,
            deadline: u64::MAX,
        }
    }

    fn execute_input(order: &pallet_rails::Order<AccountId>) -> Vec<u8> {
        encode_with_selector(
            selector_u32("execute(uint32,bytes32,bytes)"),
            (
                BASE,
                portal(),
                UnboundedBytes::from(rails_wire::encode_order(order)),
            ),
        )
    }

    /// The gateway delivers `order`. Returns the status and the amount the precompile
    /// reports.
    fn deliver(order: &pallet_rails::Order<AccountId>) -> (u8, u64) {
        let output = execute_precompile(
            &precompiles::<RailsPrecompile<Runtime>>(),
            rails(),
            gateway(),
            execute_input(order),
            U256::zero(),
        )
        .unwrap()
        .unwrap()
        .output;
        // The third word names the bridge of the order's portal.
        assert_eq!(U256::from_big_endian(&output[64..96]).low_u32(), BASE);
        (
            U256::from_big_endian(&output[..32]).low_u64() as u8,
            U256::from_big_endian(&output[32..64]).low_u64(),
        )
    }

    /// A chain with the rails ready: the bridge to Base, the gateway, a subnet whose
    /// service takes credits, the canonical pool with an established price, canonical TAO
    /// and a market for the subnet.
    fn rails_ext() -> sp_io::TestExternalities {
        let mut ext = new_test_ext();
        ext.execute_with(|| {
            Timestamp::set_timestamp(1_000_000);
            assert_ok!(Tusd::set_bridge(
                RuntimeOrigin::root(),
                BASE,
                pallet_tusd::BridgeRoute {
                    origin: OriginChain::Evm(8453),
                    domain: BASE,
                    portal: portal(),
                    asset: StableAsset::Usdc,
                },
                TusdBalance::from(u64::MAX / 4),
                TusdBalance::from(u64::MAX / 4),
                1,
            ));
            assert_ok!(Tusd::set_bridge_enabled(RuntimeOrigin::root(), BASE, true));
            // The gateway's account exists, and holds nothing to pay the transport with.
            fund_account(&mapped_account(gateway()), 1);
            assert_ok!(Rails::set_gateway(
                RuntimeOrigin::root(),
                Some(mapped_account(gateway()))
            ));
            // The hub pays up to a thousandth of a TAO for a message to Base, and Base's
            // account holds that for a hundred messages.
            assert_ok!(Rails::set_message_reserve(
                RuntimeOrigin::root(),
                BASE,
                RESERVE.into(),
                RESERVE.into()
            ));
            fund_account(&account(0x12), UNIT);
            assert_ok!(Rails::fund_messages(
                RuntimeOrigin::signed(account(0x12)),
                BASE,
                (100 * RESERVE + 1).into()
            ));

            // A subnet with a deep pool, gamma and a listed service.
            let netuid = NetUid::from(NETUID);
            SubtensorModule::init_new_network(netuid, 100);
            pallet_subtensor::SubtokenEnabled::<Runtime>::insert(netuid, true);
            pallet_subtensor::FirstEmissionBlockNumber::<Runtime>::insert(netuid, 0);
            pallet_subtensor::SubnetTAO::<Runtime>::insert(netuid, TaoBalance::from(10_000 * UNIT));
            pallet_subtensor::SubnetAlphaIn::<Runtime>::insert(
                netuid,
                AlphaBalance::from(20_000 * UNIT),
            );
            fund_account(
                &SubtensorModule::get_subnet_account_id(netuid).unwrap(),
                10_000 * UNIT,
            );
            pallet_subtensor::GammaEnabled::<Runtime>::put(true);
            pallet_subtensor::SubnetGammaEnabled::<Runtime>::insert(netuid, true);
            pallet_subtensor::TaoUsdPriceOracle::<Runtime>::put(account(0x0C));
            assert_ok!(SubtensorModule::feed_tao_usd_price(
                RuntimeOrigin::signed(account(0x0C)),
                PRICE
            ));
            pallet_subtensor::SubnetGammaPrice::<Runtime>::insert(
                netuid,
                <pallet_subtensor_swap::Pallet<Runtime> as SwapHandler>::current_alpha_price(
                    netuid,
                ),
            );
            pallet_subtensor::Services::<Runtime>::insert(
                netuid,
                account(0x5E),
                pallet_subtensor::ServiceInfo {
                    url: BoundedVec::truncate_from(b"https://service.example".to_vec()),
                },
            );

            // The canonical pool: 1,000 TAO and 400,000 tUSD, with a price history.
            assert_ok!(CanonicalPool::set_enabled(RuntimeOrigin::root(), true));
            assert_ok!(CanonicalPool::set_swap_caps(
                RuntimeOrigin::root(),
                pallet_canonical_pool::SwapCaps {
                    per_swap: Permill::from_percent(50),
                    per_window: Permill::one(),
                    window: 1,
                }
            ));
            fund_account(&account(0x11), 1_001 * UNIT);
            assert_ok!(Tusd::mint(BASE, &account(0x11), (400_000 * UNIT).into()));
            assert_ok!(CanonicalPool::add_liquidity(
                RuntimeOrigin::signed(account(0x11)),
                (1_000 * UNIT).into(),
                (400_000 * UNIT).into(),
                0
            ));
            System::set_block_number(40);
            assert_ok!(SubtensorModule::feed_tao_usd_price(
                RuntimeOrigin::signed(account(0x0C)),
                PRICE
            ));

            assert_ok!(Rails::register_token(
                RuntimeOrigin::root(),
                BASE,
                SpokeAsset::Tao,
                H160::repeat_byte(0x7A)
            ));
            assert_ok!(Rails::set_tao_cap(
                RuntimeOrigin::root(),
                BASE,
                (1_000 * UNIT).into()
            ));
            assert_ok!(Rails::open_market(
                RuntimeOrigin::root(),
                netuid,
                (1_000_000 * UNIT).into()
            ));
            assert_ok!(Rails::list_market(
                RuntimeOrigin::root(),
                netuid,
                BASE,
                H160::repeat_byte(0x64)
            ));
        });
        ext
    }

    /// The head of Base's queue: its nonce and its body as the portal reads it.
    fn next_outbound() -> Option<(u64, Vec<u8>)> {
        Rails::next_outbound(BASE)
            .map(|(_, _, outbound)| (outbound.nonce, rails_wire::encode_message(&outbound)))
    }

    fn next_outbound_input() -> Vec<u8> {
        encode_with_selector(selector_u32("nextOutbound(uint32)"), (BASE,))
    }

    /// What the transport charges for one message in these tests, in the EVM's unit: a
    /// little less than the reserve, and not a whole number of the hub's unit.
    const CHARGE_WEI: u128 = 900_000_000_000_001;
    /// That charge in the hub's unit, rounded up.
    const CHARGE: u64 = 900_001;

    /// The gateway takes message `nonce` of Base's queue, for which the transport charges
    /// `CHARGE_WEI`.
    fn ack(caller: H160, nonce: u64) -> bool {
        ack_paying(caller, nonce, U256::from(CHARGE_WEI))
    }

    fn ack_paying(caller: H160, nonce: u64, paid: U256) -> bool {
        execute_precompile(
            &precompiles::<RailsPrecompile<Runtime>>(),
            rails(),
            caller,
            encode_with_selector(
                selector_u32("ackOutbound(uint32,uint64,uint256)"),
                (BASE, nonce, paid),
            ),
            U256::zero(),
        )
        .unwrap()
        .is_ok()
    }

    #[test]
    fn the_gateway_settles_an_order_and_reads_its_receipt() {
        rails_ext().execute_with(|| {
            let order = order(
                0,
                SpokeAsset::Stable,
                250 * UNIT,
                Destination::NativeTusd { to: account(0x21) },
            );

            assert_eq!(deliver(&order), (STATUS_SETTLED, 250 * UNIT));

            assert_eq!(Tusd::balance(&account(0x21)), TusdBalance::from(250 * UNIT));
            precompiles::<RailsPrecompile<Runtime>>()
                .prepare_test(
                    stranger(),
                    rails(),
                    encode_with_selector(selector_u32("receipt(uint32,uint64)"), (BASE, 0u64)),
                )
                .with_static_call(true)
                .execute_returns((STATUS_SETTLED, 250 * UNIT, 40u32));
            precompiles::<RailsPrecompile<Runtime>>()
                .prepare_test(
                    stranger(),
                    rails(),
                    encode_with_selector(selector_u32("nextOrder(uint32)"), (BASE,)),
                )
                .with_static_call(true)
                .execute_returns(1u64);
            // An order the hub has not seen has no receipt.
            precompiles::<RailsPrecompile<Runtime>>()
                .prepare_test(
                    stranger(),
                    rails(),
                    encode_with_selector(selector_u32("receipt(uint32,uint64)"), (BASE, 1u64)),
                )
                .with_static_call(true)
                .execute_returns((STATUS_NONE, 0u64, 0u32));
        });
    }

    #[test]
    fn only_the_gateway_can_deliver_or_take_messages() {
        rails_ext().execute_with(|| {
            let order = order(
                0,
                SpokeAsset::Stable,
                250 * UNIT,
                Destination::NativeTusd { to: account(0x21) },
            );
            let root = sp_io::storage::root(sp_runtime::StateVersion::V1);
            let refused = execute_precompile(
                &precompiles::<RailsPrecompile<Runtime>>(),
                rails(),
                stranger(),
                execute_input(&order),
                U256::zero(),
            )
            .unwrap();
            assert_eq!(
                refused,
                Err(PrecompileFailure::Error {
                    exit_status: ExitError::Other("dispatch execution failed: NotGateway".into()),
                })
            );
            assert!(!ack(stranger(), 0));
            assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), root);
            assert_eq!(Tusd::balance(&account(0x21)), TusdBalance::from(0));
        });
    }

    #[test]
    fn the_hub_hands_the_gateway_what_the_transport_charges_up_to_its_limit() {
        rails_ext().execute_with(|| {
            // Listing the market queued the token's registration.
            let (nonce, _) = next_outbound().unwrap();
            let wei = |tao: u64| U256::from(tao) * U256::from(1_000_000_000u64);
            let gateway_account = mapped_account(gateway());
            let held = Balances::free_balance(&gateway_account);
            let budget = Rails::message_budget(BASE);
            let root = sp_io::storage::root(sp_runtime::StateVersion::V1);
            // The sixth word that `nextOutbound` returns: the most the hub pays now.
            let limit = || {
                let output = execute_precompile(
                    &precompiles::<RailsPrecompile<Runtime>>(),
                    rails(),
                    stranger(),
                    next_outbound_input(),
                    U256::zero(),
                )
                .unwrap()
                .unwrap()
                .output;
                U256::from_big_endian(&output[5 * 32..6 * 32])
            };

            // The hub pays no more than the reserve for a message. The transport charges
            // more, by the smallest amount: the hub pays nothing, and the message stays.
            assert_eq!(limit(), wei(RESERVE));
            assert!(!ack_paying(gateway(), nonce, wei(RESERVE) + U256::one()));
            assert!(!ack_paying(gateway(), nonce, U256::MAX));
            assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), root);
            assert_eq!(next_outbound().unwrap().0, nonce);

            // The admin lets the hub pay up to two reserves, from what Base's account can
            // spare. The gateway reads the new limit, and the message leaves at it: the
            // hub moves that much from Base's account to the gateway, which pays the
            // transport with it.
            assert_ok!(Rails::set_message_reserve(
                RuntimeOrigin::root(),
                BASE,
                RESERVE.into(),
                (2 * RESERVE).into()
            ));
            assert_eq!(limit(), wei(2 * RESERVE));
            assert!(!ack_paying(
                gateway(),
                nonce,
                wei(2 * RESERVE) + U256::one()
            ));
            assert!(ack_paying(gateway(), nonce, wei(2 * RESERVE)));
            assert_eq!(
                Balances::free_balance(&gateway_account),
                held + TaoBalance::from(2 * RESERVE)
            );
            assert_eq!(
                Rails::message_budget(BASE),
                budget - TaoBalance::from(2 * RESERVE)
            );
            assert_eq!(next_outbound(), None);
        });
    }

    #[test]
    fn the_gateway_defers_a_message_and_sends_it_later() {
        rails_ext().execute_with(|| {
            // Listing the market queued the token's registration. An order queues a credit
            // behind it.
            let (first, registration) = next_outbound().unwrap();
            let bought = deliver(&order(
                0,
                SpokeAsset::Stable,
                4_000 * UNIT,
                Destination::Tao { to: wallet() },
            ));
            assert_eq!(bought.0, STATUS_SETTLED);
            let defer = |caller: H160, nonce: u64| {
                execute_precompile(
                    &precompiles::<RailsPrecompile<Runtime>>(),
                    rails(),
                    caller,
                    encode_with_selector(
                        selector_u32("deferOutbound(uint32,uint64)"),
                        (BASE, nonce),
                    ),
                    U256::zero(),
                )
                .unwrap()
                .is_ok()
            };
            let outbound_input = |nonce: u64| {
                encode_with_selector(selector_u32("outbound(uint32,uint64)"), (BASE, nonce))
            };
            let nothing = (
                false,
                0u32,
                H256::zero(),
                UnboundedBytes::from(Vec::new()),
                U256::zero(),
            );
            let limit = U256::from(RESERVE) * U256::from(1_000_000_000u64);

            // The gateway can take the head, and no message behind it.
            precompiles::<RailsPrecompile<Runtime>>()
                .prepare_test(stranger(), rails(), outbound_input(first + 1))
                .with_static_call(true)
                .execute_returns(nothing.clone());

            // Only the gateway defers a message, and only the head.
            let root = sp_io::storage::root(sp_runtime::StateVersion::V1);
            assert!(!defer(stranger(), first));
            assert!(!defer(gateway(), first + 1));
            assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), root);
            assert!(defer(gateway(), first));

            // The credit is the head now. The registration has not left: the gateway
            // reads it by its nonce.
            let (second, credit) = next_outbound().unwrap();
            assert_eq!(second, first + 1);
            precompiles::<RailsPrecompile<Runtime>>()
                .prepare_test(stranger(), rails(), outbound_input(first))
                .with_static_call(true)
                .execute_returns((
                    true,
                    BASE,
                    portal(),
                    UnboundedBytes::from(registration),
                    limit,
                ));
            precompiles::<RailsPrecompile<Runtime>>()
                .prepare_test(stranger(), rails(), outbound_input(second))
                .with_static_call(true)
                .execute_returns((true, BASE, portal(), UnboundedBytes::from(credit), limit));

            // The credit leaves first, and the registration after it. Each leaves once.
            assert!(ack(gateway(), second));
            assert!(!ack(stranger(), first));
            assert!(ack(gateway(), first));
            assert!(!ack(gateway(), first));
            for nonce in [first, second] {
                precompiles::<RailsPrecompile<Runtime>>()
                    .prepare_test(stranger(), rails(), outbound_input(nonce))
                    .with_static_call(true)
                    .execute_returns(nothing.clone());
            }
            assert_eq!(next_outbound(), None);
            assert_ok!(Rails::do_try_state());
        });
    }

    #[test]
    fn bytes_that_are_not_an_order_are_refused() {
        rails_ext().execute_with(|| {
            let mut body = rails_wire::encode_order(&order(
                0,
                SpokeAsset::Stable,
                UNIT,
                Destination::NativeTusd { to: account(0x21) },
            ));
            // Another version.
            *body.get_mut(31).unwrap() = 2;
            for bytes in [body.clone(), body[..body.len() - 32].to_vec(), Vec::new()] {
                let result = execute_precompile(
                    &precompiles::<RailsPrecompile<Runtime>>(),
                    rails(),
                    gateway(),
                    encode_with_selector(
                        selector_u32("execute(uint32,bytes32,bytes)"),
                        (BASE, portal(), UnboundedBytes::from(bytes)),
                    ),
                    U256::zero(),
                )
                .unwrap();
                assert_eq!(
                    result,
                    Err(PrecompileFailure::Error {
                        exit_status: ExitError::Other("not an order".into()),
                    })
                );
            }
            assert_eq!(pallet_rails::NextOrder::<Runtime>::get(BASE), 0);
        });
    }

    #[test]
    fn the_gateway_reads_the_outbox_and_drains_it_in_order() {
        rails_ext().execute_with(|| {
            // Listing the market queued the token's registration.
            let (nonce, body) = next_outbound().unwrap();
            assert_eq!(nonce, 0);
            precompiles::<RailsPrecompile<Runtime>>()
                .prepare_test(stranger(), rails(), next_outbound_input())
                .with_static_call(true)
                .execute_returns((
                    true,
                    0u64,
                    BASE,
                    portal(),
                    UnboundedBytes::from(body),
                    U256::from(RESERVE) * U256::from(1_000_000_000u64),
                ));
            let budget = Rails::message_budget(BASE);
            assert!(ack(gateway(), 0));
            // Base's account paid what the transport charged, rounded up to the hub's unit.
            assert_eq!(
                Rails::message_budget(BASE),
                budget - TaoBalance::from(CHARGE)
            );

            // An order for canonical TAO queues a credit.
            let bought = deliver(&order(
                0,
                SpokeAsset::Stable,
                4_000 * UNIT,
                Destination::Tao { to: wallet() },
            ));
            assert_eq!(bought.0, STATUS_SETTLED);
            assert_eq!(
                Rails::next_outbound(BASE).unwrap().2,
                Outbound {
                    bridge: BASE,
                    nonce: 1,
                    message: Message::Credit {
                        asset: SpokeAsset::Tao,
                        to: wallet(),
                        amount: bought.1,
                        order: Some(0),
                        refund: false,
                        rate: 0,
                    },
                }
            );
            assert_eq!(
                Balances::free_balance(Rails::reserve_account()),
                TaoBalance::from(bought.1)
            );
            // Only the head leaves.
            assert!(!ack(gateway(), 2));
            assert!(ack(gateway(), 1));
            precompiles::<RailsPrecompile<Runtime>>()
                .prepare_test(stranger(), rails(), next_outbound_input())
                .with_static_call(true)
                .execute_returns((
                    false,
                    0u64,
                    0u32,
                    H256::zero(),
                    UnboundedBytes::from(Vec::new()),
                    U256::zero(),
                ));
        });
    }

    #[test]
    fn a_stablecoin_order_buys_alpha_tokens_and_sells_them_back() {
        rails_ext().execute_with(|| {
            let netuid = NetUid::from(NETUID);
            let (status, shares) = deliver(&order(
                0,
                SpokeAsset::Stable,
                4_000 * UNIT,
                Destination::Alpha {
                    netuid,
                    to: wallet(),
                },
            ));
            assert_eq!(status, STATUS_SETTLED);

            // The vault staked at the subnet's index hotkey, and one share is one alpha.
            let vault = Rails::vault_account(netuid);
            let staked = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &SubtensorModule::index_hotkey(netuid),
                &vault,
                netuid,
            );
            assert_eq!(u64::from(staked), shares);
            // The mock subnet trades alpha one for one: 4,000 dollars are about ten TAO.
            assert!(shares > 9 * UNIT && shares < 10 * UNIT, "{shares}");
            precompiles::<RailsPrecompile<Runtime>>()
                .prepare_test(
                    stranger(),
                    rails(),
                    encode_with_selector(selector_u32("alphaRate(uint16)"), (NETUID,)),
                )
                .with_static_call(true)
                .execute_returns(U256::from(1_000_000_000_000_000_000u128));

            // The holder burns the tokens on Base for the stablecoin.
            let (status, usdc) = deliver(&order(
                1,
                SpokeAsset::Alpha(netuid),
                shares,
                Destination::Stable { to: wallet() },
            ));
            assert_eq!(status, STATUS_SETTLED);
            // Less the fees and the price impact of two trips through two pools.
            assert!(usdc > 3_900 * UNIT && usdc < 4_000 * UNIT, "{usdc}");
            assert_eq!(pallet_rails::AlphaSupply::<Runtime>::get(netuid, BASE), 0);
            assert_ok!(Rails::do_try_state());
        });
    }

    #[test]
    fn credits_are_spent_for_the_beneficiary() {
        rails_ext().execute_with(|| {
            let netuid = NetUid::from(NETUID);
            let order = order(
                0,
                SpokeAsset::Stable,
                100 * UNIT,
                Destination::Credits {
                    netuid,
                    service: account(0x5E),
                    beneficiary: wallet(),
                },
            );
            let gamma =
                Rails::quote_order(BASE, order.source, order.amount, order.destination.clone())
                    .unwrap();
            assert!(gamma > 98 * UNIT && gamma < 100 * UNIT, "{gamma}");

            precompiles::<RailsPrecompile<Runtime>>()
                .prepare_test(gateway(), rails(), execute_input(&order))
                .expect_no_logs()
                .execute_returns((STATUS_SETTLED, gamma, BASE));

            // The service reads the total of the address.
            precompiles::<RailsPrecompile<Runtime>>()
                .prepare_test(
                    stranger(),
                    rails(),
                    encode_with_selector(
                        selector_u32("creditsOf(uint16,bytes32,address)"),
                        (NETUID, H256::repeat_byte(0x5E), Address(wallet())),
                    ),
                )
                .with_static_call(true)
                .execute_returns(gamma);
            // Nothing is paid out for credits, and nothing is left in the hub.
            assert_eq!(pallet_rails::NextMessage::<Runtime>::get(BASE), 1);
            assert_ok!(Rails::do_try_state());
        });
    }

    #[test]
    fn a_quote_is_what_the_order_delivers_and_changes_nothing() {
        rails_ext().execute_with(|| {
            let netuid = NetUid::from(NETUID);
            let quote = |order: &pallet_rails::Order<AccountId>| {
                encode_with_selector(
                    selector_u32("quote(uint32,bytes)"),
                    (BASE, UnboundedBytes::from(rails_wire::encode_order(order))),
                )
            };
            let buy = order(
                0,
                SpokeAsset::Stable,
                4_000 * UNIT,
                Destination::Alpha {
                    netuid,
                    to: wallet(),
                },
            );
            let root = sp_io::storage::root(sp_runtime::StateVersion::V1);
            let output = execute_precompile(
                &precompiles::<RailsPrecompile<Runtime>>(),
                rails(),
                stranger(),
                quote(&buy),
                U256::zero(),
            )
            .unwrap()
            .unwrap()
            .output;
            assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), root);
            let quoted = U256::from_big_endian(&output[32..64]).low_u64();
            assert_eq!(U256::from_big_endian(&output[..32]), U256::one());
            assert_eq!(deliver(&buy), (STATUS_SETTLED, quoted));

            // An order the hub would refund quotes its reason.
            let pointless = order(
                1,
                SpokeAsset::Stable,
                UNIT,
                Destination::Stable { to: wallet() },
            );
            precompiles::<RailsPrecompile<Runtime>>()
                .prepare_test(stranger(), rails(), quote(&pointless))
                .with_static_call(true)
                .execute_returns((false, 0u64, UnboundedString::from("InvalidRoute")));
        });
    }

    #[test]
    fn a_quote_costs_the_gas_of_settling_an_order() {
        rails_ext().execute_with(|| {
            let order = order(
                0,
                SpokeAsset::Stable,
                UNIT,
                Destination::NativeTusd { to: account(0x21) },
            );
            let weight = <Runtime as pallet_rails::Config>::WeightInfo::execute()
                .saturating_add(<Runtime as pallet_rails::Config>::Staking::weight());
            precompiles::<RailsPrecompile<Runtime>>()
                .prepare_test(
                    stranger(),
                    rails(),
                    encode_with_selector(
                        selector_u32("quote(uint32,bytes)"),
                        (BASE, UnboundedBytes::from(rails_wire::encode_order(&order))),
                    ),
                )
                .with_static_call(true)
                .expect_cost(
                    <Runtime as pallet_evm::Config>::GasWeightMapping::weight_to_gas(weight),
                )
                .execute_returns((true, UNIT, UnboundedString::from("")));
        });
    }

    #[test]
    fn a_refunded_order_reports_its_status() {
        rails_ext().execute_with(|| {
            let mut order = order(
                0,
                SpokeAsset::Stable,
                4_000 * UNIT,
                Destination::Tao { to: wallet() },
            );
            order.min_out = 100 * UNIT;
            assert_eq!(deliver(&order), (STATUS_REFUNDED, 0));
            // The refund follows the token's registration in the outbox.
            assert!(ack(gateway(), 0));
            assert_eq!(
                Rails::next_outbound(BASE).unwrap().2.message,
                Message::Credit {
                    asset: SpokeAsset::Stable,
                    to: order.payer,
                    amount: 4_000 * UNIT,
                    order: Some(0),
                    refund: true,
                    rate: 0,
                }
            );
            // A burn the hub never let the spoke mint is rejected.
            let unbacked = super::tests::order(
                1,
                SpokeAsset::Tao,
                UNIT,
                Destination::NativeTao { to: account(0x21) },
            );
            assert_eq!(deliver(&unbacked), (STATUS_REJECTED, 0));
        });
    }
}
