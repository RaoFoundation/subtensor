//! The wire format of the canonical rails between a portal on another chain and the hub.
//!
//! Both directions are Solidity ABI encodings, so the contracts use `abi.encode` and
//! `abi.decode` and never see SCALE. This module is the only place that converts between
//! those bytes and the hub's types.
//!
//! ## An order, from a portal to the hub
//!
//! ```text
//! abi.encode(
//!     uint8   version,            // ORDER_VERSION
//!     uint64  nonce,              // the portal's sequence number of the order
//!     address payer,              // who paid on the spoke; a refund returns to it
//!     uint8   source,             // 0 stablecoin, 1 canonical TAO, 2 alpha token
//!     uint16  sourceNetuid,       // the subnet of an alpha token, else 0
//!     uint64  amount,             // of the source, with 9 decimals
//!     uint8   destination,        // 0 stablecoin, 1 canonical TAO, 2 alpha token,
//!                                 // 3 service credits, 4 tUSD on Bittensor, 5 TAO on Bittensor
//!     uint16  destinationNetuid,  // the subnet of an alpha token or of credits, else 0
//!     bytes32 recipient,          // 0..2: an address, left-padded; 3: the service's
//!                                 // account; 4, 5: an account on Bittensor
//!     address beneficiary,        // 3: the address the credits are for, else 0
//!     uint64  minOut,             // the least of the destination the payer accepts
//!     uint64  deadline            // Unix seconds after which the order is refunded
//! )
//! ```
//!
//! Twelve words. An order has one encoding: a field that its kinds do not use must be
//! zero, and every value must fit its type. Anything else does not decode.
//!
//! ## A message, from the hub to a portal
//!
//! ```text
//! abi.encode(uint8 kind = 1, uint64 nonce, uint8 asset, uint16 netuid, address to,
//!            uint64 amount, bool answersOrder, uint64 order, bool refund, uint128 rate,
//!            uint32 slots)
//! abi.encode(uint8 kind = 2, uint64 nonce, uint16 netuid, address token,
//!            string name, string symbol)
//! abi.encode(uint8 kind = 3, uint64 nonce, uint32 slots)
//! ```
//!
//! `kind` 1 pays `amount` of an asset to `to`; `kind` 2 creates a subnet's alpha token.
//! `asset` and `netuid` name an asset as `source` and `sourceNetuid` do in an order.
//! `slots` is how many more orders the portal may take: `kind` 1 brings them with a
//! payment, and `kind` 3 brings them alone.

use alloc::vec::Vec;

use pallet_rails::{Destination, Message, Order, Outbound, SpokeAsset};
use sp_core::{H160, H256};
use subtensor_runtime_common::NetUid;

/// The version of the order encoding this runtime reads.
pub const ORDER_VERSION: u8 = 1;
/// The length of an encoded order: twelve words.
pub const ORDER_LEN: usize = 384;
/// The `kind` of a message that pays an asset out.
pub const KIND_CREDIT: u8 = 1;
/// The `kind` of a message that creates an alpha token.
pub const KIND_REGISTER_TOKEN: u8 = 2;
/// The `kind` of a message that only brings order slots.
pub const KIND_SLOTS: u8 = 3;

const WORD: usize = 32;

/// Reads the words of an encoding, each strictly as its type.
struct Words<'a>(core::slice::ChunksExact<'a, u8>);

impl<'a> Words<'a> {
    fn word(&mut self) -> Option<&'a [u8]> {
        self.0.next()
    }

    /// An unsigned integer of `bytes` bytes: the rest of the word must be zero.
    fn uint(&mut self, bytes: usize) -> Option<u64> {
        let (padding, value) = self.word()?.split_at_checked(WORD.checked_sub(bytes)?)?;
        if padding.iter().any(|byte| *byte != 0) {
            return None;
        }
        let mut buffer = [0u8; 8];
        buffer
            .get_mut(8usize.checked_sub(bytes)?..)?
            .copy_from_slice(value);
        Some(u64::from_be_bytes(buffer))
    }

    fn address(&mut self) -> Option<H160> {
        address(self.word()?)
    }

    fn bytes32(&mut self) -> Option<[u8; 32]> {
        self.word()?.try_into().ok()
    }
}

/// The address in a word: its upper twelve bytes must be zero.
fn address(word: &[u8]) -> Option<H160> {
    let (padding, value) = word.split_at_checked(12)?;
    padding
        .iter()
        .all(|byte| *byte == 0)
        .then(|| H160::from_slice(value))
}

fn asset(kind: u64, netuid: u16) -> Option<SpokeAsset> {
    match (kind, netuid) {
        (0, 0) => Some(SpokeAsset::Stable),
        (1, 0) => Some(SpokeAsset::Tao),
        (2, netuid) => Some(SpokeAsset::Alpha(NetUid::from(netuid))),
        _ => None,
    }
}

fn asset_code(asset: SpokeAsset) -> (u8, u16) {
    match asset {
        SpokeAsset::Stable => (0, 0),
        SpokeAsset::Tao => (1, 0),
        SpokeAsset::Alpha(netuid) => (2, netuid.into()),
    }
}

/// Decodes an order. `None` if `body` is not the one encoding of an order.
pub fn decode_order<AccountId: From<[u8; 32]>>(body: &[u8]) -> Option<Order<AccountId>> {
    if body.len() != ORDER_LEN {
        return None;
    }
    let mut words = Words(body.chunks_exact(WORD));
    if words.uint(1)? != u64::from(ORDER_VERSION) {
        return None;
    }
    let nonce = words.uint(8)?;
    let payer = words.address()?;
    let source = asset(words.uint(1)?, u16::try_from(words.uint(2)?).ok()?)?;
    let amount = words.uint(8)?;
    let destination_kind = words.uint(1)?;
    let destination_netuid = u16::try_from(words.uint(2)?).ok()?;
    let recipient = words.bytes32()?;
    let beneficiary = words.address()?;
    let min_out = words.uint(8)?;
    let deadline = words.uint(8)?;

    let netuid = NetUid::from(destination_netuid);
    let destination = match (destination_kind, destination_netuid, beneficiary.is_zero()) {
        (0, 0, true) => Destination::Stable {
            to: address(&recipient)?,
        },
        (1, 0, true) => Destination::Tao {
            to: address(&recipient)?,
        },
        (2, _, true) => Destination::Alpha {
            netuid,
            to: address(&recipient)?,
        },
        (3, _, _) => Destination::Credits {
            netuid,
            service: recipient.into(),
            beneficiary,
        },
        (4, 0, true) => Destination::NativeTusd {
            to: recipient.into(),
        },
        (5, 0, true) => Destination::NativeTao {
            to: recipient.into(),
        },
        _ => return None,
    };
    Some(Order {
        nonce,
        payer,
        source,
        amount,
        destination,
        min_out,
        deadline,
    })
}

/// Writes the words of an encoding.
#[derive(Default)]
struct Encoder(Vec<u8>);

impl Encoder {
    fn uint(mut self, value: u128) -> Self {
        self.0.extend_from_slice(&[0u8; 16]);
        self.0.extend_from_slice(&value.to_be_bytes());
        self
    }

    fn address(mut self, value: H160) -> Self {
        self.0.extend_from_slice(&[0u8; 12]);
        self.0.extend_from_slice(value.as_bytes());
        self
    }

    fn bytes32(mut self, value: [u8; 32]) -> Self {
        self.0.extend_from_slice(&value);
        self
    }

    /// The tail of a `string`: its length, then its bytes padded to whole words.
    fn string(mut self, value: &[u8]) -> Self {
        self = self.uint(value.len() as u128);
        self.0.extend_from_slice(value);
        self.0.resize(self.0.len().next_multiple_of(WORD), 0);
        self
    }
}

/// The words a `string` of `length` bytes takes in a tail, in bytes.
fn string_len(length: usize) -> usize {
    WORD.saturating_add(length.next_multiple_of(WORD))
}

/// Encodes an order as a portal does. The hub only decodes orders; this is for clients and
/// tests.
pub fn encode_order<AccountId: Into<[u8; 32]> + Clone>(order: &Order<AccountId>) -> Vec<u8> {
    let (source, source_netuid) = asset_code(order.source);
    let padded = |to: &H160| H256::from(*to).0;
    let (kind, netuid, recipient, beneficiary) = match &order.destination {
        Destination::Stable { to } => (0, 0, padded(to), H160::zero()),
        Destination::Tao { to } => (1, 0, padded(to), H160::zero()),
        Destination::Alpha { netuid, to } => (2, u16::from(*netuid), padded(to), H160::zero()),
        Destination::Credits {
            netuid,
            service,
            beneficiary,
        } => (3, u16::from(*netuid), service.clone().into(), *beneficiary),
        Destination::NativeTusd { to } => (4, 0, to.clone().into(), H160::zero()),
        Destination::NativeTao { to } => (5, 0, to.clone().into(), H160::zero()),
    };
    Encoder::default()
        .uint(ORDER_VERSION.into())
        .uint(order.nonce.into())
        .address(order.payer)
        .uint(source.into())
        .uint(source_netuid.into())
        .uint(order.amount.into())
        .uint(kind)
        .uint(netuid.into())
        .bytes32(recipient)
        .address(beneficiary)
        .uint(order.min_out.into())
        .uint(order.deadline.into())
        .0
}

/// Encodes a message of the hub for its portal.
pub fn encode_message(outbound: &Outbound) -> Vec<u8> {
    match &outbound.message {
        Message::Credit {
            asset,
            to,
            amount,
            order,
            refund,
            rate,
            slots,
        } => {
            let (asset, netuid) = asset_code(*asset);
            Encoder::default()
                .uint(KIND_CREDIT.into())
                .uint(outbound.nonce.into())
                .uint(asset.into())
                .uint(netuid.into())
                .address(*to)
                .uint((*amount).into())
                .uint(order.is_some().into())
                .uint(order.unwrap_or_default().into())
                .uint((*refund).into())
                .uint(*rate)
                .uint((*slots).into())
                .0
        }
        Message::RegisterToken {
            netuid,
            token,
            name,
            symbol,
        } => {
            // Six words of head; the two strings follow, each at its offset.
            let name_at = WORD.saturating_mul(6);
            let symbol_at = name_at.saturating_add(string_len(name.len()));
            Encoder::default()
                .uint(KIND_REGISTER_TOKEN.into())
                .uint(outbound.nonce.into())
                .uint(u16::from(*netuid).into())
                .address(*token)
                .uint(name_at as u128)
                .uint(symbol_at as u128)
                .string(name)
                .string(symbol)
                .0
        }
        Message::Slots { count } => {
            Encoder::default()
                .uint(KIND_SLOTS.into())
                .uint(outbound.nonce.into())
                .uint((*count).into())
                .0
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::indexing_slicing, clippy::unwrap_used)]

    use super::*;
    use precompile_utils::{
        prelude::{Address, UnboundedString},
        solidity::encode_arguments,
    };

    type Account = [u8; 32];

    fn order(destination: Destination<Account>) -> Order<Account> {
        Order {
            nonce: 7,
            payer: H160::repeat_byte(0xAA),
            source: SpokeAsset::Stable,
            amount: 250_000_000_000,
            destination,
            min_out: 1_000,
            deadline: 1_900_000_000,
        }
    }

    fn destinations() -> Vec<Destination<Account>> {
        let to = H160::repeat_byte(0xBB);
        vec![
            Destination::Stable { to },
            Destination::Tao { to },
            Destination::Alpha {
                netuid: NetUid::from(64),
                to,
            },
            Destination::Credits {
                netuid: NetUid::from(51),
                service: [0xCC; 32],
                beneficiary: to,
            },
            Destination::NativeTusd { to: [0xDD; 32] },
            Destination::NativeTao { to: [0xEE; 32] },
        ]
    }

    #[test]
    fn every_order_round_trips() {
        for destination in destinations() {
            for source in [
                SpokeAsset::Stable,
                SpokeAsset::Tao,
                SpokeAsset::Alpha(NetUid::from(1)),
                SpokeAsset::Alpha(NetUid::from(u16::MAX)),
            ] {
                let mut order = order(destination.clone());
                order.source = source;
                let body = encode_order(&order);
                assert_eq!(body.len(), ORDER_LEN);
                assert_eq!(decode_order::<Account>(&body), Some(order));
            }
        }
        // The largest values fit.
        let mut largest = order(Destination::NativeTao { to: [0xFF; 32] });
        largest.nonce = u64::MAX;
        largest.amount = u64::MAX;
        largest.min_out = u64::MAX;
        largest.deadline = u64::MAX;
        assert_eq!(
            decode_order::<Account>(&encode_order(&largest)),
            Some(largest)
        );
    }

    /// The encoder is `abi.encode` of the twelve fields: a contract that encodes an order
    /// with Solidity produces these bytes.
    #[test]
    fn an_order_is_the_abi_encoding_of_its_fields() {
        let order = order(Destination::Credits {
            netuid: NetUid::from(51),
            service: [0xCC; 32],
            beneficiary: H160::repeat_byte(0xBB),
        });
        let expected = encode_arguments((
            ORDER_VERSION,
            7u64,
            Address(H160::repeat_byte(0xAA)),
            0u8,
            0u16,
            250_000_000_000u64,
            3u8,
            51u16,
            H256::repeat_byte(0xCC),
            Address(H160::repeat_byte(0xBB)),
            1_000u64,
            1_900_000_000u64,
        ));
        assert_eq!(encode_order(&order), expected);
    }

    #[test]
    fn an_order_of_the_wrong_length_or_version_does_not_decode() {
        let body = encode_order(&order(Destination::Stable {
            to: H160::repeat_byte(0xBB),
        }));
        assert!(decode_order::<Account>(&body).is_some());
        assert_eq!(decode_order::<Account>(&body[..ORDER_LEN - 1]), None);
        assert_eq!(
            decode_order::<Account>(&[body.clone(), vec![0]].concat()),
            None
        );
        assert_eq!(
            decode_order::<Account>(&[body.clone(), vec![0; 32]].concat()),
            None
        );
        assert_eq!(decode_order::<Account>(&[]), None);
        for version in [0u8, 2, 255] {
            let mut other = body.clone();
            other[31] = version;
            assert_eq!(decode_order::<Account>(&other), None);
        }
    }

    /// One order, one encoding: a value that overflows its type, or a field that the
    /// order's kinds do not use, makes the bytes something else than an order.
    #[test]
    fn an_order_has_one_encoding() {
        // The byte just above each field's type, by word.
        let widths = [1usize, 8, 20, 1, 2, 8, 1, 2, 32, 20, 8, 8];
        for destination in destinations() {
            let body = encode_order(&order(destination));
            for (word, width) in widths.iter().enumerate() {
                if *width == 32 {
                    continue;
                }
                let mut dirty = body.clone();
                dirty[word * 32 + 31 - width] = 1;
                assert_eq!(decode_order::<Account>(&dirty), None, "word {word}");
            }
        }

        let set = |body: &[u8], word: usize, value: u8| {
            let mut changed = body.to_vec();
            changed[word * 32 + 31] = value;
            changed
        };
        let stable = encode_order(&order(Destination::Stable {
            to: H160::repeat_byte(0xBB),
        }));
        // A netuid on a source or a destination that has none.
        assert_eq!(decode_order::<Account>(&set(&stable, 4, 1)), None);
        assert_eq!(decode_order::<Account>(&set(&stable, 7, 1)), None);
        // A beneficiary on an order that buys no credits.
        assert_eq!(decode_order::<Account>(&set(&stable, 9, 1)), None);
        // An address with bytes above its twenty.
        let mut wide = stable.clone();
        wide[8 * 32] = 1;
        assert_eq!(decode_order::<Account>(&wide), None);
        // Kinds that do not exist.
        assert_eq!(decode_order::<Account>(&set(&stable, 3, 3)), None);
        assert_eq!(decode_order::<Account>(&set(&stable, 6, 6)), None);
        // An account on Bittensor may be any 32 bytes; an address may not.
        for kind in [4u8, 5] {
            let mut native = set(&stable, 6, kind);
            native[8 * 32] = 1;
            assert!(decode_order::<Account>(&native).is_some());
        }
    }

    #[test]
    fn a_credit_is_the_abi_encoding_of_its_fields() {
        let to = H160::repeat_byte(0xBB);
        let credit = |asset, order, refund, rate| Outbound {
            bridge: 8453,
            nonce: 9,
            message: Message::Credit {
                asset,
                to,
                amount: 123_456_789,
                order,
                refund,
                rate,
                slots: 5,
            },
        };
        let expected = |asset: u8, netuid: u16, answers: bool, order: u64, refund: bool, rate| {
            encode_arguments((
                KIND_CREDIT,
                9u64,
                asset,
                netuid,
                Address(to),
                123_456_789u64,
                answers,
                order,
                refund,
                rate,
                5u32,
            ))
        };
        assert_eq!(
            encode_message(&credit(SpokeAsset::Stable, Some(4), true, 0)),
            expected(0, 0, true, 4, true, 0u128)
        );
        assert_eq!(
            encode_message(&credit(SpokeAsset::Tao, None, false, 0)),
            expected(1, 0, false, 0, false, 0u128)
        );
        assert_eq!(
            encode_message(&credit(
                SpokeAsset::Alpha(NetUid::from(64)),
                Some(0),
                false,
                u128::MAX
            )),
            expected(2, 64, true, 0, false, u128::MAX)
        );
        // Eleven words.
        assert_eq!(
            encode_message(&credit(SpokeAsset::Tao, None, false, 0)).len(),
            352
        );
    }

    #[test]
    fn a_token_registration_is_the_abi_encoding_of_its_fields() {
        for (name, symbol) in [
            ("Chutes", "CHUTES"),
            ("", ""),
            // Thirty-two bytes fill a word; thirty-three start another.
            ("a name of thirty-two bytes long!", "BLOCKMACHIN"),
            ("a name of thirty-three bytes long", "X"),
            ("τ-vanta", "VANTA"),
        ] {
            let outbound = Outbound {
                bridge: 8453,
                nonce: 3,
                message: Message::RegisterToken {
                    netuid: NetUid::from(64),
                    token: H160::repeat_byte(0x64),
                    name: name.as_bytes().to_vec().try_into().unwrap(),
                    symbol: symbol.as_bytes().to_vec().try_into().unwrap(),
                },
            };
            let expected = encode_arguments((
                KIND_REGISTER_TOKEN,
                3u64,
                64u16,
                Address(H160::repeat_byte(0x64)),
                UnboundedString::from(name),
                UnboundedString::from(symbol),
            ));
            assert_eq!(encode_message(&outbound), expected, "{name}");
        }
    }

    /// The bytes the contracts' tests decode: `contracts/evm/test/Wire.t.sol` holds the
    /// same vectors.
    #[test]
    fn golden_vectors() {
        let order = order(Destination::Alpha {
            netuid: NetUid::from(64),
            to: H160::repeat_byte(0xBB),
        });
        assert_eq!(
            hex::encode(encode_order(&order)),
            concat!(
                "0000000000000000000000000000000000000000000000000000000000000001",
                "0000000000000000000000000000000000000000000000000000000000000007",
                "000000000000000000000000aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "0000000000000000000000000000000000000000000000000000000000000000",
                "0000000000000000000000000000000000000000000000000000000000000000",
                "0000000000000000000000000000000000000000000000000000003a35294400",
                "0000000000000000000000000000000000000000000000000000000000000002",
                "0000000000000000000000000000000000000000000000000000000000000040",
                "000000000000000000000000bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "0000000000000000000000000000000000000000000000000000000000000000",
                "00000000000000000000000000000000000000000000000000000000000003e8",
                "00000000000000000000000000000000000000000000000000000000713fb300",
            )
        );
        let credit = Outbound {
            bridge: 8453,
            nonce: 9,
            message: Message::Credit {
                asset: SpokeAsset::Alpha(NetUid::from(64)),
                to: H160::repeat_byte(0xBB),
                amount: 123_456_789,
                order: Some(7),
                refund: false,
                rate: 1_100_000_000_000_000_000,
                slots: 2,
            },
        };
        assert_eq!(
            hex::encode(encode_message(&credit)),
            concat!(
                "0000000000000000000000000000000000000000000000000000000000000001",
                "0000000000000000000000000000000000000000000000000000000000000009",
                "0000000000000000000000000000000000000000000000000000000000000002",
                "0000000000000000000000000000000000000000000000000000000000000040",
                "000000000000000000000000bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "00000000000000000000000000000000000000000000000000000000075bcd15",
                "0000000000000000000000000000000000000000000000000000000000000001",
                "0000000000000000000000000000000000000000000000000000000000000007",
                "0000000000000000000000000000000000000000000000000000000000000000",
                "0000000000000000000000000000000000000000000000000f43fc2c04ee0000",
                "0000000000000000000000000000000000000000000000000000000000000002",
            )
        );
        let register = Outbound {
            bridge: 8453,
            nonce: 3,
            message: Message::RegisterToken {
                netuid: NetUid::from(64),
                token: H160::repeat_byte(0x64),
                name: b"Chutes".to_vec().try_into().unwrap(),
                symbol: b"CHUTES".to_vec().try_into().unwrap(),
            },
        };
        assert_eq!(
            hex::encode(encode_message(&register)),
            concat!(
                "0000000000000000000000000000000000000000000000000000000000000002",
                "0000000000000000000000000000000000000000000000000000000000000003",
                "0000000000000000000000000000000000000000000000000000000000000040",
                "0000000000000000000000006464646464646464646464646464646464646464",
                "00000000000000000000000000000000000000000000000000000000000000c0",
                "0000000000000000000000000000000000000000000000000000000000000100",
                "0000000000000000000000000000000000000000000000000000000000000006",
                "4368757465730000000000000000000000000000000000000000000000000000",
                "0000000000000000000000000000000000000000000000000000000000000006",
                "4348555445530000000000000000000000000000000000000000000000000000",
            )
        );
        let slots = Outbound {
            bridge: 8453,
            nonce: 11,
            message: Message::Slots { count: 64 },
        };
        assert_eq!(
            hex::encode(encode_message(&slots)),
            concat!(
                "0000000000000000000000000000000000000000000000000000000000000003",
                "000000000000000000000000000000000000000000000000000000000000000b",
                "0000000000000000000000000000000000000000000000000000000000000040",
            )
        );
    }

    #[test]
    fn slots_are_the_abi_encoding_of_their_count() {
        let slots = Outbound {
            bridge: 8453,
            nonce: 12,
            message: Message::Slots { count: u32::MAX },
        };
        assert_eq!(
            encode_message(&slots),
            encode_arguments((KIND_SLOTS, 12u64, u32::MAX))
        );
    }
}
