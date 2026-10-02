//! Public registration challenge mining. GPU drivers are optional and loaded at
//! runtime; no signing material enters the miner or device memory.

mod opencl;

use sha2::{Digest, Sha256};
use sp_core::{hashing::keccak_256, U256};

pub use opencl::{gpu_devices, GpuDevice, GpuMiner};

pub const PREFIX_LEN: usize = 123;
pub const MAX_BATCH: u32 = 1_048_576;
pub type Solution = (u64, [u8; 32]);

pub fn seal(prefix: &[u8], nonce: u64) -> Result<[u8; 32], String> {
    validate_prefix(prefix)?;
    let mut hasher = Sha256::new();
    hasher.update(prefix);
    hasher.update(nonce.to_le_bytes());
    Ok(keccak_256(&hasher.finalize()))
}

fn validate_prefix(prefix: &[u8]) -> Result<(), String> {
    if prefix.len() != PREFIX_LEN || !prefix.starts_with(b"subtensor-pow-register-v1") {
        return Err("expected the 123-byte subnet registration challenge prefix".into());
    }
    if prefix.get(25..27) == Some(&[0, 0]) {
        return Err("PoW registration is unavailable on root".into());
    }
    Ok(())
}

fn target(difficulty: u64) -> Result<U256, String> {
    if difficulty == 0 {
        return Err("PoW difficulty must be positive".into());
    }
    U256::MAX
        .checked_div(U256::from(difficulty))
        .ok_or_else(|| "PoW difficulty must be positive".into())
}

fn validate_batch(prefix: &[u8], difficulty: u64, attempts: u32) -> Result<U256, String> {
    validate_prefix(prefix)?;
    if attempts == 0 || attempts > MAX_BATCH {
        return Err(format!("PoW batch must contain 1..{MAX_BATCH} attempts"));
    }
    target(difficulty)
}

/// Bounded Rust CPU fallback; callers refresh the chain challenge between batches.
pub fn mine_cpu(
    prefix: &[u8],
    difficulty: u64,
    start: u64,
    attempts: u32,
) -> Result<Option<Solution>, String> {
    let limit = validate_batch(prefix, difficulty, attempts)?;
    for offset in 0..attempts {
        let nonce = start.wrapping_add(u64::from(offset));
        let work = seal(prefix, nonce)?;
        if U256::from_little_endian(&work) <= limit {
            return Ok(Some((nonce, work)));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]
    use super::*;

    pub(super) fn prefix() -> Vec<u8> {
        let mut bytes = b"subtensor-pow-register-v1".to_vec();
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&[1u8; 32]);
        bytes.extend_from_slice(&[2u8; 32]);
        bytes.extend_from_slice(&[3u8; 32]);
        bytes
    }

    #[test]
    fn seal_matches_independent_python_vectors() {
        let prefix = prefix();
        for (nonce, expected) in [
            (
                0,
                "7cdaced691a70795a7656ad9b76ef2c16b961686a14a790bcb22eed6226ef103",
            ),
            (
                1,
                "c2291b632879447b6fe0785b0f25b5151c85a7e07df23a7860cf029e8a03f466",
            ),
            (
                u64::MAX,
                "69f1aef50783893195365b4fb62a4745085c31fee525a0984bc20301ab377aaf",
            ),
        ] {
            assert_eq!(hex::encode(seal(&prefix, nonce).unwrap()), expected);
        }
    }

    #[test]
    fn cpu_mining_preserves_nonce_and_little_endian_target() {
        let prefix = prefix();
        let solution = mine_cpu(&prefix, 1, u64::MAX, 2).unwrap().unwrap();
        assert_eq!(solution.0, u64::MAX);
        assert_eq!(solution.1, seal(&prefix, u64::MAX).unwrap());
        assert_eq!(target(2).unwrap(), U256::MAX / U256::from(2));
        assert_eq!(target(u64::MAX).unwrap(), U256::MAX / U256::from(u64::MAX));
    }

    #[test]
    #[ignore = "requires physical OpenCL GPUs; run explicitly on the GPU test host"]
    fn gpu_mines_and_revalidates_chain_compatible_work() {
        let mut miner = GpuMiner::new(None).expect("hardware test requires GPUs");
        assert!(!miner.devices().is_empty());
        let prefix = prefix();
        for start in [0, u64::MAX - 100] {
            let (nonce, work) = miner.mine(&prefix, 10, start, 4096).unwrap().unwrap();
            assert_eq!(work, seal(&prefix, nonce).unwrap());
            assert!(U256::from_little_endian(&work) <= target(10).unwrap());
        }
    }

    #[test]
    fn invalid_challenges_and_unbounded_batches_are_rejected() {
        let prefix = prefix();
        assert!(mine_cpu(&prefix, 0, 0, 1).is_err());
        assert!(mine_cpu(&prefix, 1, 0, 0).is_err());
        assert!(mine_cpu(&prefix, 1, 0, MAX_BATCH + 1).is_err());
        assert!(mine_cpu(&prefix[..122], 1, 0, 1).is_err());
        let mut root = prefix.clone();
        root[25..27].fill(0);
        assert!(seal(&root, 1).is_err());
    }
}
