//! Transaction extension that raises mempool priority for unsigned [`Call::write_pulse`].

use crate::{Call, Config};
use codec::{Decode, DecodeWithMemTracking, Encode};
use frame_support::dispatch::{DispatchInfo, PostDispatchInfo};
use frame_support::pallet_prelude::Weight;
use frame_support::traits::IsSubType;
use scale_info::TypeInfo;
use sp_runtime::traits::{
    DispatchInfoOf, DispatchOriginOf, Dispatchable, Implication, TransactionExtension,
    ValidateResult,
};
use sp_runtime::transaction_validity::{
    TransactionPriority, TransactionSource, TransactionValidityError, ValidTransaction,
};
use sp_std::marker::PhantomData;
use subtensor_macros::freeze_struct;

/// Runtime call type alias used by [`DrandPriority`].
pub type RuntimeCallFor<T> = <T as frame_system::Config>::RuntimeCall;

/// Signed-extension marker that bumps priority when the call is [`Call::write_pulse`].
///
/// Does not alter other calls; weight is currently zero pending a dedicated benchmark.
#[freeze_struct("fafde7074128b670")]
#[derive(Default, Encode, Decode, DecodeWithMemTracking, Clone, Eq, PartialEq, TypeInfo)]
pub struct DrandPriority<T: Config + Send + Sync + TypeInfo>(pub PhantomData<T>);

impl<T: Config + Send + Sync + TypeInfo> sp_std::fmt::Debug for DrandPriority<T> {
    fn fmt(&self, f: &mut sp_std::fmt::Formatter) -> sp_std::fmt::Result {
        write!(f, "DrandPriority")
    }
}

impl<T: Config + Send + Sync + TypeInfo> DrandPriority<T> {
    /// Construct the extension (stateless; `PhantomData` only).
    pub fn new() -> Self {
        Self(PhantomData)
    }

    /// Fixed priority applied to `write_pulse` in `validate`.
    fn unsigned_write_pulse_priority() -> TransactionPriority {
        10_000u64
    }
}

impl<T: Config + Send + Sync + TypeInfo> TransactionExtension<RuntimeCallFor<T>>
    for DrandPriority<T>
where
    <T as frame_system::Config>::RuntimeCall:
        Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>,
    <T as frame_system::Config>::RuntimeCall: IsSubType<Call<T>>,
{
    const IDENTIFIER: &'static str = "DrandPriority";
    type Implicit = ();
    type Val = ();
    type Pre = ();

    fn weight(&self, _call: &RuntimeCallFor<T>) -> Weight {
        // TODO: benchmark transaction extension
        Weight::zero()
    }

    fn validate(
        &self,
        origin: DispatchOriginOf<RuntimeCallFor<T>>,
        call: &RuntimeCallFor<T>,
        _info: &DispatchInfoOf<RuntimeCallFor<T>>,
        _len: usize,
        _self_implicit: Self::Implicit,
        _inherited_implication: &impl Implication,
        _source: TransactionSource,
    ) -> ValidateResult<Self::Val, RuntimeCallFor<T>> {
        match call.is_sub_type() {
            Some(Call::write_pulse { .. }) => {
                let validity = ValidTransaction {
                    priority: Self::unsigned_write_pulse_priority(),
                    ..Default::default()
                };

                Ok((validity, (), origin))
            }
            _ => Ok((Default::default(), (), origin)),
        }
    }

    fn prepare(
        self,
        _val: Self::Val,
        _origin: &DispatchOriginOf<RuntimeCallFor<T>>,
        _call: &RuntimeCallFor<T>,
        _info: &DispatchInfoOf<RuntimeCallFor<T>>,
        _len: usize,
    ) -> Result<Self::Pre, TransactionValidityError> {
        Ok(())
    }
}
