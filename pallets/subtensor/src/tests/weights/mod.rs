#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]
//! Tests for subnet weight extrinsics and helpers in [`crate::subnets::weights`].
//!
//! ## Search anchors
//!
//! | Module | Owns |
//! |--------|------|
//! | [`helpers`] | `commit_reveal_set_weights` fixture |
//! | [`set_weights`] | `set_weights` dispatch, stake/permit/version guards |
//! | [`weight_checks`] | `check_length`, normalize, max-weight, self-weight, epoch block helper |
//! | [`commit_reveal`] | hash commit–reveal happy path & toggles |
//! | [`commit_reveal_timing`] | expiry, exact epoch/block, tempo changes |
//! | [`batch_reveal`] | `batch_reveal_weights` + batch event netuid fields |
//! | [`timelocked_commit`] | CRv3 / timelocked commits + tlock smoke |
//! | [`timelocked_reveal`] | CRv3 reveal failure modes and multi-commit processing |
//! | [`timelocked_reveal_hotkey`] | CRv3 hotkey check, missing-pulse retry, legacy payload |
//! | [`owner_permit`] | subnet-owner validate without stake/permit |

mod batch_reveal;
mod commit_reveal;
mod commit_reveal_timing;
mod helpers;
mod owner_permit;
mod set_weights;
mod timelocked_commit;
mod timelocked_reveal;
mod timelocked_reveal_hotkey;
mod weight_checks;
