#![allow(
    unused,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used
)]
//! Unit tests for coinbase emission, drain, and dividend distribution.
//!
//! Split from the former monolithic `tests/coinbase.rs` into concept modules.
//!
//! ## Search anchors
//!
//! | Module | Owns |
//! |--------|------|
//! | [`helpers`] | `close` / `set_full_injection_root_stake` fixtures |
//! | [`tao_issuance`] | TAO issuance and emission-enable redistribution |
//! | [`moving_price`] | Moving-price updates |
//! | [`alpha_issuance`] | Alpha issuance and cap triggers |
//! | [`owner_cut`] | Subnet owner cut |
//! | [`pending_emission`] | Pending emission accumulation |
//! | [`drain_emission`] | Drain pending emission to stakers / childkeys |
//! | [`root_children_drain`] | Root children dividend drain |
//! | [`incentive_burn`] | Incentive burn / burn-key sorting |
//! | [`dividend_distribution`] | Dividend and incentive distribution math |
//! | [`distribute_emission`] | Distribute-emission edge cases |
//! | [`run_coinbase_lifecycle`] | run_coinbase start-block gating |
//! | [`incentive_autostake`] | Incentive autostake destination |
//! | [`mining_emission`] | Mining emission with/without root sell |
//! | [`subnet_terms`] | Subnet terms / registration gates |
//! | [`inject_and_swap`] | Inject-and-maybe-swap / TAO materialization |
//! | [`drain_pending_epoch`] | BlocksSinceLastStep / epoch deferral |
//! | [`emit_to_subnets`] | emit_to_subnets root-sell variants |
//! | [`root_proportion`] | Root proportion bookkeeping |
//! | [`epoch_cap_deferral`] | Epoch cap deferral / CRV3 reveal |
//! | [`alpha_dividends`] | Alpha dividend collateral / take floor |

mod alpha_dividends;
mod alpha_issuance;
mod distribute_emission;
mod dividend_distribution;
mod drain_emission;
mod drain_pending_epoch;
mod emit_to_subnets;
mod epoch_cap_deferral;
mod helpers;
mod incentive_autostake;
mod incentive_burn;
mod inject_and_swap;
mod mining_emission;
mod moving_price;
mod owner_cut;
mod pending_emission;
mod prelude;
mod root_children_drain;
mod root_proportion;
mod run_coinbase_lifecycle;
mod subnet_terms;
mod tao_issuance;
