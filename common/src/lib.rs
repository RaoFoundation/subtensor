//! Shared runtime types for Subtensor: subnet IDs, balances, proxy filters, and cross-pallet traits.
//!
//! Crate name is `subtensor-runtime-common`. Prefer this module for `NetUid`, `TaoBalance`,
//! `AlphaBalance`, and proxy metadata rather than raw `u16`/`u64` at API boundaries.
#![cfg_attr(not(feature = "std"), no_std)]
use core::fmt::{self, Display, Formatter};

use codec::{
    Compact, CompactAs, Decode, DecodeWithMemTracking, Encode, Error as CodecError, MaxEncodedLen,
};
use frame_support::pallet_prelude::*;
use runtime_common::prod_or_fast;
use scale_info::TypeInfo;
use serde::{Deserialize, Serialize};
use sp_runtime::{
    MultiSignature, Vec,
    traits::{IdentifyAccount, Verify},
};

pub use sp_io::MultiRemovalResults;
use subtensor_macros::freeze_struct;

pub use currency::*;
pub use evm_context::*;
pub use proxy::*;
pub use transaction_error::*;

use frame_support::weights::WeightMeter;

mod currency;
mod evm_context;
mod proxy;
mod transaction_error;

/// Account free-balance type; alias of [`TaoBalance`] (RAO / 1e9 TAO units).
pub type Balance = TaoBalance;

/// An index to a block.
pub type BlockNumber = u32;

/// Alias to 512-bit hash when used in the context of a transaction signature on the chain.
pub type Signature = MultiSignature;

/// Some way of identifying an account on the chain. We intentionally make it equivalent to the
/// public key of our transaction signing scheme.
pub type AccountId = <<Signature as Verify>::Signer as IdentifyAccount>::AccountId;

/// Index of a transaction in the chain.
pub type Index = u32;

/// A hash of some data used by the chain.
pub type Hash = sp_core::H256;

/// Account extrinsic nonce (same width as [`Index`]).
pub type Nonce = u32;

/// Max TAO amount (inclusive bound exclusive of limit) treated as a "small" proxy transfer: 0.5 TAO.
pub const SMALL_TRANSFER_LIMIT: Balance = TaoBalance::new(500_000_000); // 0.5 TAO
/// Max alpha amount treated as a "small" proxy stake transfer: 0.5 alpha (same RAO scale as TAO).
pub const SMALL_ALPHA_TRANSFER_LIMIT: AlphaBalance = AlphaBalance::new(500_000_000); // 0.5 Alpha

/// Subnet identifier (`netuid`): opaque `u16` newtype used as the canonical subnet key.
///
/// `0` is the root network ([`NetUid::ROOT`]). SCALE-identical to `u16`; metadata keeps the path
/// so SDKs emit a distinct type rather than a bare integer.
#[freeze_struct("a4e7d2e5110c5c67")]
#[repr(transparent)]
#[derive(
    Deserialize,
    Serialize,
    Clone,
    Copy,
    Decode,
    DecodeWithMemTracking,
    Default,
    Encode,
    Eq,
    Hash,
    MaxEncodedLen,
    Ord,
    PartialEq,
    PartialOrd,
    RuntimeDebug,
    TypeInfo,
)]
#[serde(transparent)]
pub struct NetUid(u16);

impl NetUid {
    /// Root network (`netuid == 0`); emission/weight parent of non-root subnets.
    pub const ROOT: NetUid = Self(0);

    /// Returns `true` when this is the root network.
    pub fn is_root(&self) -> bool {
        *self == Self::ROOT
    }

    /// Next netuid (`saturating_add(1)`); used when allocating sequential subnet IDs.
    pub fn next(&self) -> NetUid {
        Self(self.0.saturating_add(1))
    }

    /// Previous netuid (`saturating_sub(1)`).
    pub fn prev(&self) -> NetUid {
        Self(self.0.saturating_sub(1))
    }

    /// Inner `u16` for storage math that still takes raw integers.
    pub fn inner(&self) -> u16 {
        self.0
    }
}

impl Display for NetUid {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl CompactAs for NetUid {
    type As = u16;

    fn encode_as(&self) -> &Self::As {
        &self.0
    }

    fn decode_from(v: Self::As) -> Result<Self, CodecError> {
        Ok(Self(v))
    }
}

impl From<Compact<NetUid>> for NetUid {
    fn from(c: Compact<NetUid>) -> Self {
        c.0
    }
}

impl From<NetUid> for u16 {
    fn from(val: NetUid) -> Self {
        val.0
    }
}

impl From<u16> for NetUid {
    fn from(value: u16) -> Self {
        Self(value)
    }
}

/// Read-only subnet metadata used by swap, leases, and other pallets that must not depend on
/// the full subtensor pallet.
pub trait SubnetInfo<AccountId> {
    /// Whether a subnet with this netuid exists.
    fn exists(netuid: NetUid) -> bool;
    /// Subnet mechanism / consensus mode code for `netuid`.
    fn mechanism(netuid: NetUid) -> u16;
    /// Whether `account_id` is the subnet owner coldkey.
    fn is_owner(account_id: &AccountId, netuid: NetUid) -> bool;
    /// Whether the subnet's alpha (subtoken) is enabled for trading/staking.
    fn is_subtoken_enabled(netuid: NetUid) -> bool;
    /// Per-uid validator trust vector for the subnet.
    fn get_validator_trust(netuid: NetUid) -> Vec<u16>;
    /// Per-uid validator permit flags for the subnet.
    fn get_validator_permit(netuid: NetUid) -> Vec<bool>;
    /// Hotkey registered at `uid` on `netuid`, if any.
    fn hotkey_of_uid(netuid: NetUid, uid: u16) -> Option<AccountId>;
}

/// AMM / pool reserve accessors for a subnet's TAO or alpha side (`C: Token`).
pub trait TokenReserve<C: Token> {
    /// Current reserve amount for `netuid`.
    fn reserve(netuid: NetUid) -> C;
    /// Credit supply provided into the reserve (liquidity in).
    fn increase_provided(netuid: NetUid, amount: C);
    /// Debit supply provided from the reserve (liquidity out).
    fn decrease_provided(netuid: NetUid, amount: C);
}

/// Cross-pallet stake and free-balance operations used by swap and limit-order flows.
pub trait BalanceOps<AccountId> {
    /// Free TAO balance of `account_id`.
    fn tao_balance(account_id: &AccountId) -> TaoBalance;
    /// Alpha stake of (`coldkey`, `hotkey`) on `netuid`.
    fn alpha_balance(netuid: NetUid, coldkey: &AccountId, hotkey: &AccountId) -> AlphaBalance;
    /// Increase alpha stake for (`coldkey`, `hotkey`) on `netuid`.
    fn increase_stake(
        coldkey: &AccountId,
        hotkey: &AccountId,
        netuid: NetUid,
        alpha: AlphaBalance,
    ) -> Result<(), DispatchError>;
    /// Decrease alpha stake for (`coldkey`, `hotkey`) on `netuid`.
    fn decrease_stake(
        coldkey: &AccountId,
        hotkey: &AccountId,
        netuid: NetUid,
        alpha: AlphaBalance,
    ) -> Result<(), DispatchError>;
}

/// Allows to query the current block author
pub trait AuthorshipInfo<AccountId> {
    /// Return the current block author
    fn author() -> Option<AccountId>;
}

/// Block-time constants shared by the runtime (slot duration, minutes/hours/days in blocks).
pub mod time {
    use super::*;

    /// This determines the average expected block time that we are targeting. Blocks will be
    /// produced at a minimum duration defined by `SLOT_DURATION`. `SLOT_DURATION` is picked up by
    /// `pallet_timestamp` which is in turn picked up by `pallet_aura` to implement `fn
    /// slot_duration()`.
    ///
    /// Change this to adjust the block time.
    pub const MILLISECS_PER_BLOCK: u64 = prod_or_fast!(12000, 250);

    // NOTE: Currently it is not possible to change the slot duration after the chain has started.
    //       Attempting to do so will brick block production.
    pub const SLOT_DURATION: u64 = MILLISECS_PER_BLOCK;

    // Time is measured by number of blocks.
    pub const MINUTES: BlockNumber = 60_000 / (MILLISECS_PER_BLOCK as BlockNumber);
    pub const HOURS: BlockNumber = MINUTES * 60;
    pub const DAYS: BlockNumber = HOURS * 24;
}

/// Sub-subnet / mechanism identifier within a parent [`NetUid`].
///
/// `0` ([`MechId::MAIN`]) is the primary mechanism. Combined with netuid into
/// [`NetUidStorageIndex`] for per-mechanism epoch maps (`index = netuid + mecid * 4096`).
#[freeze_struct("56b975909205713f")]
#[repr(transparent)]
#[derive(
    Deserialize,
    Serialize,
    Clone,
    Copy,
    Decode,
    DecodeWithMemTracking,
    Default,
    Encode,
    Eq,
    Hash,
    MaxEncodedLen,
    Ord,
    PartialEq,
    PartialOrd,
    RuntimeDebug,
    TypeInfo,
)]
#[serde(transparent)]
pub struct MechId(u8);

impl MechId {
    /// Primary mechanism for a subnet (`mecid == 0`).
    pub const MAIN: MechId = Self(0);
}

impl From<u8> for MechId {
    fn from(value: u8) -> Self {
        Self(value)
    }
}

impl From<MechId> for u16 {
    fn from(val: MechId) -> Self {
        Into::<u16>::into(val.0)
    }
}

impl From<MechId> for u64 {
    fn from(val: MechId) -> Self {
        Into::<u64>::into(val.0)
    }
}

impl From<MechId> for u8 {
    fn from(val: MechId) -> Self {
        Into::<u8>::into(val.0)
    }
}

impl Display for MechId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl CompactAs for MechId {
    type As = u8;

    fn encode_as(&self) -> &Self::As {
        &self.0
    }

    fn decode_from(v: Self::As) -> Result<Self, CodecError> {
        Ok(Self(v))
    }
}

impl From<Compact<MechId>> for MechId {
    fn from(c: Compact<MechId>) -> Self {
        c.0
    }
}

/// Storage key for per-mechanism epoch maps (weights, bonds, incentives, …).
///
/// Encoding: `netuid + mecid * GLOBAL_MAX_SUBNET_COUNT` (4096). For `mecid == 0` this equals
/// [`NetUid`], preserving pre-mechanism storage layout. Prefer
/// `Pallet::get_mechanism_storage_index` over hand-rolled arithmetic.
#[freeze_struct("ca534527653f6010")]
#[repr(transparent)]
#[derive(
    Deserialize,
    Serialize,
    Clone,
    Copy,
    Decode,
    DecodeWithMemTracking,
    Default,
    Encode,
    Eq,
    Hash,
    MaxEncodedLen,
    Ord,
    PartialEq,
    PartialOrd,
    RuntimeDebug,
    TypeInfo,
)]
#[serde(transparent)]
pub struct NetUidStorageIndex(u16);

impl NetUidStorageIndex {
    /// Root-network storage index (`0`), same as [`NetUid::ROOT`].
    pub const ROOT: NetUidStorageIndex = Self(0);
}

impl Display for NetUidStorageIndex {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl CompactAs for NetUidStorageIndex {
    type As = u16;

    fn encode_as(&self) -> &Self::As {
        &self.0
    }

    fn decode_from(v: Self::As) -> Result<Self, CodecError> {
        Ok(Self(v))
    }
}

impl From<Compact<NetUidStorageIndex>> for NetUidStorageIndex {
    fn from(c: Compact<NetUidStorageIndex>) -> Self {
        c.0
    }
}

impl From<NetUid> for NetUidStorageIndex {
    fn from(val: NetUid) -> Self {
        val.0.into()
    }
}

impl From<NetUidStorageIndex> for u16 {
    fn from(val: NetUidStorageIndex) -> Self {
        val.0
    }
}

impl From<u16> for NetUidStorageIndex {
    fn from(value: u16) -> Self {
        Self(value)
    }
}

/// Clears as many entries as the weight budget allows via `clear_prefix`, charging
/// `per_item` for each removed entry. Returns `true` once the prefix is fully cleared.
pub fn clear_prefix_with_meter(
    meter: &mut WeightMeter,
    per_item: Weight,
    clear_prefix: impl FnOnce(u32) -> MultiRemovalResults,
) -> bool {
    let Some(limit) = meter.remaining().checked_div_per_component(&per_item) else {
        return false;
    };
    // Saturate: a budget allowing more than u32::MAX removals is capped, not rejected.
    let limit = u32::try_from(limit).unwrap_or(u32::MAX);

    if limit == 0 {
        return false;
    }

    let result = clear_prefix(limit);
    meter.consume(per_item.saturating_mul(result.unique.max(result.loops).into()));

    result.maybe_cursor.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use frame_support::{
        Blake2_128Concat, storage::types::StorageDoubleMap, traits::StorageInstance,
        weights::WeightMeter,
    };
    const REF_TIME_WEIGHT: u64 = 100;
    const PROOF_SIZE_WEIGHT: u64 = 100;

    struct ClearPrefixTestStorage;

    impl StorageInstance for ClearPrefixTestStorage {
        fn pallet_prefix() -> &'static str {
            "CommonTests"
        }

        const STORAGE_PREFIX: &'static str = "ClearPrefixTestStorage";
    }

    type ClearPrefixTestMap =
        StorageDoubleMap<ClearPrefixTestStorage, Identity, NetUid, Blake2_128Concat, u16, u32>;

    #[test]
    fn netuid_has_u16_bin_repr() {
        assert_eq!(NetUid(5).encode(), 5u16.encode());
    }

    // The metadata must carry the newtype identities (path segments) so SDKs can
    // reconstruct NetUid/TaoBalance/AlphaBalance/... instead of seeing bare ints,
    // while the SCALE encoding stays byte-identical to the inner integer.
    #[test]
    fn newtypes_keep_identity_in_metadata_and_bin_repr() {
        use scale_info::TypeInfo;

        fn assert_path<T: TypeInfo>(expected_ident: &str) {
            let path = T::type_info().path;
            assert_eq!(path.ident(), Some(expected_ident));
        }

        assert_path::<NetUid>("NetUid");
        assert_path::<MechId>("MechId");
        assert_path::<NetUidStorageIndex>("NetUidStorageIndex");
        assert_path::<TaoBalance>("TaoBalance");
        assert_path::<AlphaBalance>("AlphaBalance");

        assert_eq!(MechId::from(7).encode(), 7u8.encode());
        assert_eq!(NetUidStorageIndex::from(9).encode(), 9u16.encode());
        assert_eq!(TaoBalance::new(11).encode(), 11u64.encode());
        assert_eq!(AlphaBalance::new(13).encode(), 13u64.encode());
    }

    #[test]
    fn clear_prefix_with_meter_respects_budget() {
        let netuid = NetUid::from(42);
        let entry_weight = Weight::from_parts(REF_TIME_WEIGHT, PROOF_SIZE_WEIGHT);
        let mut ext = sp_io::TestExternalities::default();

        ext.execute_with(|| {
            for key in 0..3 {
                ClearPrefixTestMap::insert(netuid, key, key as u32);
            }
        });

        let _ = ext.commit_all();

        ext.execute_with(|| {
            assert_eq!(ClearPrefixTestMap::iter_prefix(netuid).count(), 3);

            // Budget for exactly one entry: one entry is removed, not done yet.
            let mut weight_meter = WeightMeter::with_limit(entry_weight);
            assert!(!clear_prefix_with_meter(
                &mut weight_meter,
                entry_weight,
                |limit| ClearPrefixTestMap::clear_prefix(netuid, limit, None),
            ));

            assert_eq!(ClearPrefixTestMap::iter_prefix(netuid).count(), 2);
            assert_eq!(weight_meter.consumed(), entry_weight);
        });
    }

    #[test]
    fn clear_prefix_with_meter_zero_budget_is_noop() {
        let netuid = NetUid::from(43);
        let entry_weight = Weight::from_parts(REF_TIME_WEIGHT, PROOF_SIZE_WEIGHT);
        let mut ext = sp_io::TestExternalities::default();

        ext.execute_with(|| {
            ClearPrefixTestMap::insert(netuid, 0, 0u32);

            let mut weight_meter = WeightMeter::with_limit(Weight::zero());
            assert!(!clear_prefix_with_meter(
                &mut weight_meter,
                entry_weight,
                |limit| ClearPrefixTestMap::clear_prefix(netuid, limit, None),
            ));

            assert_eq!(ClearPrefixTestMap::iter_prefix(netuid).count(), 1);
            assert!(weight_meter.consumed().is_zero());
        });
    }

    #[test]
    fn clear_prefix_with_meter_completes_with_enough_budget() {
        let netuid = NetUid::from(44);
        let entry_weight = Weight::from_parts(REF_TIME_WEIGHT, PROOF_SIZE_WEIGHT);
        let mut ext = sp_io::TestExternalities::default();

        ext.execute_with(|| {
            for key in 0..3 {
                ClearPrefixTestMap::insert(netuid, key, key as u32);
            }
        });

        let _ = ext.commit_all();

        ext.execute_with(|| {
            // Budget for more entries than exist: everything is cleared in one call.
            let mut weight_meter = WeightMeter::with_limit(entry_weight.saturating_mul(10));
            assert!(clear_prefix_with_meter(
                &mut weight_meter,
                entry_weight,
                |limit| ClearPrefixTestMap::clear_prefix(netuid, limit, None),
            ));

            assert_eq!(ClearPrefixTestMap::iter_prefix(netuid).count(), 0);
            assert_eq!(weight_meter.consumed(), entry_weight.saturating_mul(3));
        });
    }
}
