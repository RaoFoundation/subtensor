//! Pre-dispatch guards for Subtensor extrinsics (`DispatchExtension` + shared helpers).
//!
//! These types run at every `call.dispatch(origin)` site (including nested proxy
//! dispatches), rejecting invalid signed calls before the pallet extrinsic body.
//! The signed-tx path also reuses the same `check` / `applies_to` helpers from
//! [`crate::extensions::SubtensorTransactionExtension`].
//!
//! ## Search anchors
//!
//! | Guard | Blocks / validates |
//! |-------|--------------------|
//! | [`CheckColdkeySwap`] | Non-swap calls while a coldkey swap is announced/disputed |
//! | [`CheckWeights`] | Weight batch shape, min stake, commit/reveal readiness |
//! | [`CheckRateLimits`] | Weight-set and `register_network` rate limits |
//! | [`CheckDelegateTake`] | Delegate take bounds + coldkey ownership |
//! | [`CheckServingEndpoints`] | Axon / prometheus serve preconditions |
//! | [`CheckEvmKeyAssociation`] | EVM-key association registration + cooldown |
//!
//! [`subtensor_call_if`] returns the inner [`Call`] when a guard's `applies_to`
//! predicate matches — used by weight accounting and by the transaction extension.

mod check_coldkey_swap;
mod check_delegate_take;
mod check_evm_key_association;
mod check_rate_limits;
mod check_serving_endpoints;
mod check_weights;

use crate::{Call, Config};
use frame_support::traits::IsSubType;
use sp_runtime::traits::Dispatchable;

pub use check_coldkey_swap::*;
pub use check_delegate_take::*;
pub use check_evm_key_association::*;
pub use check_rate_limits::*;
pub use check_serving_endpoints::*;
pub use check_weights::*;

/// Runtime-wide call type (`frame_system::Config::RuntimeCall`) used by guard extensions.
pub(crate) type GuardsRuntimeCallOf<T> = <T as frame_system::Config>::RuntimeCall;

/// Origin type carried by [`GuardsRuntimeCallOf`] dispatches (signed / root / none).
pub(crate) type RuntimeCallOriginOf<T> = <GuardsRuntimeCallOf<T> as Dispatchable>::RuntimeOrigin;

/// If `call` is a Subtensor [`Call`] and `applies_to` returns true, yield that call.
///
/// Returns `None` for non-Subtensor calls or Subtensor calls outside the guard's
/// scope (so the guard charges zero weight and skips `pre_dispatch` work).
pub(crate) fn subtensor_call_if<T>(
    call: &GuardsRuntimeCallOf<T>,
    applies_to: impl FnOnce(&Call<T>) -> bool,
) -> Option<&Call<T>>
where
    T: Config,
    GuardsRuntimeCallOf<T>: IsSubType<Call<T>>,
{
    let call = call.is_sub_type()?;
    applies_to(call).then_some(call)
}
