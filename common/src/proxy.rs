//! Proxy-type identifiers and client-facing call-filter metadata for pallet-proxy.
//!
//! Runtime filtering in `runtime/src/proxy_filters` remains the source of truth.
//! Types here are the on-chain / RPC view of the same allowlists (`ProxyType`,
//! [`CallInfo`], [`ProxyFilterInfo`]).

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use frame_support::traits::{Contains, GetCallIndex, GetCallName, PalletInfoAccess};
use scale_info::TypeInfo;
use sp_runtime::Vec;
use subtensor_macros::freeze_struct;

/// Stable proxy-type identifiers used on-chain and by RPC clients.
///
/// Variant order and the explicit `u8` mapping below are part of the wire /
/// announcement surface — do not reorder variants or renumber discriminants.
/// Deprecated variants ([`ProxyType::is_deprecated`]) always deny calls.
#[derive(
    Copy,
    Clone,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Encode,
    Decode,
    DecodeWithMemTracking,
    Debug,
    MaxEncodedLen,
    TypeInfo,
)]
pub enum ProxyType {
    /// Unrestricted proxy: all runtime calls allowed.
    Any,
    /// Subnet-owner call set.
    Owner,
    /// Non-critical / low-risk call set.
    NonCritical,
    /// Calls that do not move free TAO or stake.
    NonTransfer,
    /// Deprecated senate governance proxy (always denies).
    Senate,
    /// Calls that do not move fungible TAO (NFTs / non-value paths).
    NonFungible,
    /// Deprecated triumvirate governance proxy (always denies).
    Triumvirate,
    /// Deprecated governance proxy (always denies).
    Governance,
    /// Staking / unstaking / swap stake call set.
    Staking,
    /// Neuron / subnet registration call set.
    Registration,
    /// Free-balance and stake transfer call set.
    Transfer,
    /// Transfers bounded by [`crate::SMALL_TRANSFER_LIMIT`] / [`crate::SMALL_ALPHA_TRANSFER_LIMIT`].
    SmallTransfer,
    /// Deprecated root-weights proxy (always denies).
    RootWeights,
    /// Child-hotkey relationship call set.
    ChildKeys,
    /// Sudo `set_code` only (high privilege).
    SudoUncheckedSetCode,
    /// Hotkey swap call set.
    SwapHotkey,
    /// Subnet lease beneficiary call set.
    SubnetLeaseBeneficiary,
    /// Root-claim call set.
    RootClaim,
}

impl TryFrom<u8> for ProxyType {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Any),
            1 => Ok(Self::Owner),
            2 => Ok(Self::NonCritical),
            3 => Ok(Self::NonTransfer),
            4 => Ok(Self::Senate),
            5 => Ok(Self::NonFungible),
            6 => Ok(Self::Triumvirate),
            7 => Ok(Self::Governance),
            8 => Ok(Self::Staking),
            9 => Ok(Self::Registration),
            10 => Ok(Self::Transfer),
            11 => Ok(Self::SmallTransfer),
            12 => Ok(Self::RootWeights),
            13 => Ok(Self::ChildKeys),
            14 => Ok(Self::SudoUncheckedSetCode),
            15 => Ok(Self::SwapHotkey),
            16 => Ok(Self::SubnetLeaseBeneficiary),
            17 => Ok(Self::RootClaim),
            _ => Err(()),
        }
    }
}

impl From<ProxyType> for u8 {
    fn from(proxy_type: ProxyType) -> Self {
        match proxy_type {
            ProxyType::Any => 0,
            ProxyType::Owner => 1,
            ProxyType::NonCritical => 2,
            ProxyType::NonTransfer => 3,
            ProxyType::Senate => 4,
            ProxyType::NonFungible => 5,
            ProxyType::Triumvirate => 6,
            ProxyType::Governance => 7,
            ProxyType::Staking => 8,
            ProxyType::Registration => 9,
            ProxyType::Transfer => 10,
            ProxyType::SmallTransfer => 11,
            ProxyType::RootWeights => 12,
            ProxyType::ChildKeys => 13,
            ProxyType::SudoUncheckedSetCode => 14,
            ProxyType::SwapHotkey => 15,
            ProxyType::SubnetLeaseBeneficiary => 16,
            ProxyType::RootClaim => 17,
        }
    }
}

impl ProxyType {
    /// Whether this proxy type is retired and always filters to deny.
    pub fn is_deprecated(&self) -> bool {
        matches!(
            self,
            Self::Triumvirate | Self::Senate | Self::Governance | Self::RootWeights
        )
    }
}

impl Default for ProxyType {
    fn default() -> Self {
        Self::Any
    }
}

/// Extra constraint attached to an allowed call in filter metadata.
#[derive(Clone, PartialEq, Eq, Encode, Decode, Debug, TypeInfo)]
pub enum CallConstraint {
    /// The named numeric parameter must be lower than `limit`.
    ParamLessThan { param_name: Vec<u8>, limit: u128 },
    /// The named boxed call parameter must target this pallet/call pair.
    NestedCallMustBe {
        param_name: Vec<u8>,
        pallet_name: Vec<u8>,
        call_name: Vec<u8>,
    },
}

/// Runtime call identity exposed in proxy filter metadata (pallet + call + optional constraint).
#[freeze_struct("85f86877d3d9b870")]
#[derive(Clone, PartialEq, Eq, Encode, Decode, Debug, TypeInfo)]
pub struct CallInfo {
    /// Runtime pallet name.
    pub pallet_name: Vec<u8>,
    /// Runtime pallet index.
    pub pallet_index: u8,
    /// Pallet call name.
    pub call_name: Vec<u8>,
    /// Pallet call index.
    pub call_index: u8,
    /// Optional value or nested-call constraint.
    pub constraint: Option<CallConstraint>,
}

/// Builds a [`CallInfo`] for pallet `P` call named `name` (no constraint).
///
/// Panics if `name` is not a call on `P` — intended for const/filter-group construction.
pub fn call_info_by_name<P: PalletInfoAccess, C: GetCallName + GetCallIndex>(
    name: &str,
) -> CallInfo {
    let names = C::get_call_names();
    let indices = C::get_call_indices();
    let pos = names
        .iter()
        .position(|n| *n == name)
        .unwrap_or_else(|| panic!("Call '{}' not found in pallet '{}'", name, P::name()));

    CallInfo {
        pallet_name: P::name().as_bytes().to_vec(),
        pallet_index: P::index() as u8,
        call_name: name.as_bytes().to_vec(),
        call_index: indices
            .get(pos)
            .copied()
            .unwrap_or_else(|| panic!("Call '{}' index out of bounds in '{}'", name, P::name())),
        constraint: None,
    }
}

/// Metadata view for a call filter group.
///
/// Implementations should be generated from the same rules as the executable
/// filter so clients and runtime behavior cannot drift.
pub trait CallFilterMetadata {
    /// Flat list of allowed calls (and constraints) for this filter group.
    fn call_infos() -> Vec<CallInfo>;
}

/// A reusable filter group: executable predicate plus metadata for clients.
pub trait CallFilterGroup<Call>: Contains<Call> + CallFilterMetadata {}

impl<T, Call> CallFilterGroup<Call> for T where T: Contains<Call> + CallFilterMetadata {}

impl CallFilterMetadata for () {
    fn call_infos() -> Vec<CallInfo> {
        Vec::new()
    }
}

#[impl_trait_for_tuples::impl_for_tuples(1, 32)]
impl CallFilterMetadata for Tuple {
    fn call_infos() -> Vec<CallInfo> {
        let mut infos = Vec::new();
        for_tuples!( #( infos.extend(Tuple::call_infos()); )* );
        infos
    }
}

/// Public metadata model for a proxy filter allowlist shape.
#[derive(Clone, PartialEq, Eq, Encode, Decode, Debug, TypeInfo)]
pub enum FilterMode {
    /// All runtime calls are allowed.
    AllowAll,
    /// Only listed calls are allowed. An empty list means deny all.
    Allow(Vec<CallInfo>),
}

/// Runtime API response describing one [`ProxyType`]'s filter.
#[freeze_struct("13eab55e0c9576a8")]
#[derive(Clone, PartialEq, Eq, Encode, Decode, Debug, TypeInfo)]
pub struct ProxyFilterInfo {
    /// [`ProxyType`] as its stable `u8` discriminant.
    pub proxy_type: u8,
    /// Human-readable proxy type name (UTF-8 bytes).
    pub name: Vec<u8>,
    /// Whether the proxy type is deprecated and always denies.
    pub deprecated: bool,
    /// Allow-all vs allow-list filter mode.
    pub filter_mode: FilterMode,
}

/// Compact name/index/deprecated triple for listing proxy types over RPC.
#[freeze_struct("d8933caab5cdc1e")]
#[derive(Clone, PartialEq, Eq, Encode, Decode, Debug, TypeInfo)]
pub struct ProxyTypeInfo {
    /// Human-readable proxy type name (UTF-8 bytes).
    pub name: Vec<u8>,
    /// Stable `u8` discriminant matching [`ProxyType`].
    pub index: u8,
    /// Whether the proxy type is deprecated.
    pub deprecated: bool,
}
