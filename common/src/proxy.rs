use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use frame_support::traits::{Contains, GetCallIndex, GetCallName, PalletInfoAccess};
use scale_info::TypeInfo;
use sp_runtime::Vec;
use subtensor_macros::freeze_struct;

/// Shared proxy filter model exposed by the runtime API.
///
/// Runtime filtering remains the source of truth. This metadata is the client-facing
/// allowlist view of the same rules.
/// Stable proxy type identifiers used on-chain and by RPC clients.
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
    Any,
    Owner,
    NonCritical,
    NonTransfer,
    Senate,
    NonFungible, // Nothing involving moving TAO
    Triumvirate,
    Governance,
    Staking,
    Registration,
    Transfer,
    SmallTransfer,
    RootWeights, // Deprecated
    ChildKeys,
    SudoUncheckedSetCode,
    SwapHotkey,
    SubnetLeaseBeneficiary,
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

/// Extra constraint attached to an allowed call.
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

/// Runtime call identity exposed in proxy filter metadata.
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

/// Public metadata model for a proxy filter.
#[derive(Clone, PartialEq, Eq, Encode, Decode, Debug, TypeInfo)]
pub enum FilterMode {
    /// All runtime calls are allowed.
    AllowAll,
    /// Only listed calls are allowed. An empty list means deny all.
    Allow(Vec<CallInfo>),
}

/// Runtime API response for one proxy type.
#[freeze_struct("288413f4da5ab4ee")]
#[derive(Clone, PartialEq, Eq, Encode, Decode, Debug, TypeInfo)]
pub struct ProxyFilterInfo {
    pub proxy_type: u8,
    pub name: Vec<u8>,
    pub deprecated: bool,
    pub filter_mode: FilterMode,
}

#[freeze_struct("b0cce66ed9b2451b")]
#[derive(Clone, PartialEq, Eq, Encode, Decode, Debug, TypeInfo)]
pub struct ProxyTypeInfo {
    pub name: Vec<u8>,
    pub index: u8,
    pub deprecated: bool,
}
