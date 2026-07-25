//! Consensus mechanism adapters for Aura, Babe, and hybrid Aura→Babe import.
//!
//! [`ConsensusMechanism`] lets `service` / `command` stay generic while Aura and
//! Babe supply import queues, inherent providers, authorship, and RPC extras.
//! [`hybrid_import_queue`] verifies either digest style during the migration.

mod aura_consensus;
mod babe_consensus;
mod consensus_mechanism;
mod hybrid_import_queue;

pub use aura_consensus::AuraConsensus;
pub use babe_consensus::BabeConsensus;
pub use consensus_mechanism::ConsensusMechanism;
pub use consensus_mechanism::StartAuthoringParams;
