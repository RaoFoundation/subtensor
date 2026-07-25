#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]
//! Integration tests for subnet register / dissolve / prune / registration-queue.
//!
//! Split from the former monolithic `tests/networks.rs` into concept modules.
//!
//! ## Search anchors
//!
//! | Module | Owns |
//! |--------|------|
//! | [`helpers`] | dissolve pipeline + owner-alpha price fixtures |
//! | [`dissolve_refunds`] | lock refund, pro-rata TAO, protocol-alpha share |
//! | [`dissolve_storage_cleanup`] | per-subnet / mechanism / lock map purge |
//! | [`dissolve_async_cleanup`] | on_idle cleanup queue and phases |
//! | [`destroy_alpha_stakes`] | α-in/out stake destroy payouts and lock cleanup |
//! | [`prune_network`] | lowest-price prune selection and immunity |
//! | [`register_network`] | register network, lock cost, owner-alpha seeding |
//! | [`median_subnet_alpha_price`] | median α price for new subnet pool seed |
//! | [`registered_subnet_counter`] | per-netuid registration counter |
//! | [`migrate_network_immunity`] | network immunity period migration |
//! | [`set_new_network_state`] | `set_new_network_state` pool / identity / limits |
//! | [`network_registration_queue`] | deferred registration after dissolve cleanup |
//! | [`massive_dissolve_reregistration`] | lossless dissolve + re-register flow |
//! | [`tempo_rate_limit`] | tempo vs weight-set rate limit gate |

mod destroy_alpha_stakes;
mod dissolve_async_cleanup;
mod dissolve_refunds;
mod dissolve_storage_cleanup;
mod helpers;
mod massive_dissolve_reregistration;
mod median_subnet_alpha_price;
mod migrate_network_immunity;
mod network_registration_queue;
mod prelude;
mod prune_network;
mod register_network;
mod registered_subnet_counter;
mod set_new_network_state;
mod tempo_rate_limit;
