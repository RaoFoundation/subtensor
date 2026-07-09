/// Single source of truth for all compile-time constants used across the crate.

// ---------- Chain tempo bounds ----------

/// Upper bound for owner-set tempo (≈ 7 days at 12 s/block).
pub const MAX_TEMPO: u16 = 50_400;

/// `MAX_TEMPO` widened for arithmetic with `u64` epoch counters.
pub const MAX_TEMPO_U64: u64 = MAX_TEMPO as u64;

// ---------- Drand network ----------

/// Drand Quicknet BLS public key (hex-encoded).
pub const DRAND_PUBLIC_KEY: &str = "83cf0f2896adee7eb8b5f01fcad3912212c437e0073e911fb90022d3e760183c8c4b450b6a0a6c3ac6a5776a2d1064510d1fec758c921cc22b0e17e63aaf4bcb5ed66304de9cf809bd274ca73bab4af5a6e9c76a4bc09e76eae8991ef5ece45a";

/// Drand Quicknet genesis timestamp (Unix seconds).
pub const GENESIS_TIME: u64 = 1_692_803_367;

/// Drand Quicknet round period in seconds.
pub const DRAND_PERIOD: u64 = 3;

/// Drand Quicknet chain hash.
pub const QUICKNET_CHAIN_HASH: &str =
    "52db9ba70e0cc0f6eaf7803dd07447a1f5477735fd3f661792ba94600c84e971";

/// Public Drand HTTP endpoints (tried in order).
pub const DRAND_ENDPOINTS: [&str; 5] = [
    "https://api.drand.sh",
    "https://api2.drand.sh",
    "https://api3.drand.sh",
    "https://drand.cloudflare.com",
    "https://api.drand.secureweb3.com:6875",
];

// ---------- Commit simulation ----------

/// Additional blocks added to the predicted reveal block to target
/// a drand pulse that has already been ingested on-chain.
pub const SECURITY_BLOCK_OFFSET: u64 = 3;

/// Offset applied to `current_block` to account for standard mempool
/// inclusion delay: the extrinsic lands in the **next** block relative
/// to the chain head queried by the SDK.
pub const COMMIT_INCLUSION_BLOCK_OFFSET: u64 = 1;

/// Upper bound on blocks simulated when searching for the reveal block.
pub fn max_simulation_blocks(reveal_period_epochs: u64) -> u64 {
    reveal_period_epochs
        .saturating_mul(MAX_TEMPO_U64)
        .saturating_add(MAX_TEMPO_U64)
}

// ---------- ML-KEM ----------

/// XChaCha20Poly1305 nonce length used in ML-KEM seal/open.
pub const MLKEM_NONCE_LEN: usize = 24;
