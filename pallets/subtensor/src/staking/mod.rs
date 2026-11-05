use super::*;
pub mod account;
pub mod add_stake;
pub mod auto_parent;
mod basket_flush;
pub use basket_flush::BasketFlushWork;
#[cfg(test)]
pub(crate) use basket_flush::{MAX_BASKET_FLUSH_ROWS, MAX_BASKET_ROWS};
mod basket_trade;
mod deferred_dividends;
pub use deferred_dividends::{DeferredBasketConversion, DeferredBasketDebt, DeferredBasketState};
mod basket_views;
pub mod beta_pricing;
mod claim_root;
#[cfg(test)]
pub(crate) use claim_root::RootClaimOutcome;
pub mod decrease_take;
pub mod helpers;
pub mod increase_take;
pub mod lock;
pub mod move_stake;
pub mod order_swap;
pub mod recycle_alpha;
pub mod remove_stake;
pub mod set_children;
pub mod stake_utils;

/// Release-owner-authorized compatibility policy for additional authorization
/// work: retain proof accounting without increasing legacy execution prices.
/// The work still executes. This permits more actual work per block; it is a
/// capacity decision, not a benchmark or an execution-time bound.
pub fn legacy_priced_additional_work(
    weight: frame_support::weights::Weight,
) -> frame_support::weights::Weight {
    frame_support::weights::Weight::from_parts(0, weight.proof_size())
}
