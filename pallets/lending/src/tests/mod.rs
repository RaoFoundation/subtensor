#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]

mod golden;
mod hooks;
mod interest;
pub(crate) mod mock;
mod params;
mod positions;
mod prices;
mod rate;
mod rate_increase;
mod try_state;
