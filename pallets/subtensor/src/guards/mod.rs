mod check_coldkey_swap;
mod check_delegate_take;
mod check_evm_key_association;
mod check_rate_limits;
mod check_serving_endpoints;
mod check_subnet_sale;
mod check_weights;

use crate::{Call, Config};
use frame_support::traits::IsSubType;
use sp_runtime::traits::Dispatchable;

pub use check_coldkey_swap::*;
pub use check_delegate_take::*;
pub use check_evm_key_association::*;
pub use check_rate_limits::*;
pub use check_serving_endpoints::*;
pub use check_subnet_sale::*;
pub use check_weights::*;

pub(crate) type CallOf<T> = <T as frame_system::Config>::RuntimeCall;
pub(crate) type DispatchableOriginOf<T> = <CallOf<T> as Dispatchable>::RuntimeOrigin;

pub(crate) fn applicable_call<T>(
    call: &CallOf<T>,
    applies_to: impl FnOnce(&Call<T>) -> bool,
) -> Option<&Call<T>>
where
    T: Config,
    CallOf<T>: IsSubType<Call<T>>,
{
    let call = call.is_sub_type()?;
    applies_to(call).then_some(call)
}
