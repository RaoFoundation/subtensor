use stc_shield::MemoryShieldKeystore;
use stp_shield::{Result as TraitResult, ShieldKeystore};

/// A fixed (non-rotating) shield keystore for single-validator dev/manual-seal nodes.
///
/// Uses the same ML-KEM-768 keypair for both `next_enc_key()` and `current_dec_key()`,
/// bypassing the multi-validator key-rotation timing assumption. In a real multi-validator
/// AURA chain, each validator builds every Kth block (K≥3), so the keystore rolls at the
/// same cadence as the on-chain PendingKey pipeline (2-block delay). In single-validator
/// manual-seal mode the keystore would roll on every block, drifting 2 pairs ahead of
/// PendingKey. This keystore avoids that by keeping both keys from the same generated pair.
///
/// Construction: capture `next_enc_key()` from a fresh `MemoryShieldKeystore`, roll once
/// so that key becomes current, then freeze. `current_dec_key()` delegates to the inner
/// store (which now holds the matching pair), and `roll_for_next_slot()` is a no-op.
pub struct DevShieldKeystore {
    enc_key_bytes: Vec<u8>,
    inner: MemoryShieldKeystore,
}

impl DevShieldKeystore {
    #[allow(clippy::expect_used)]
    pub fn new() -> Self {
        let inner = MemoryShieldKeystore::new();
        let enc_key_bytes = inner
            .next_enc_key()
            .expect("MemoryShieldKeystore always has a next key");
        inner
            .roll_for_next_slot()
            .expect("initial roll should not fail");
        Self {
            enc_key_bytes,
            inner,
        }
    }
}

impl Default for DevShieldKeystore {
    fn default() -> Self {
        Self::new()
    }
}

impl ShieldKeystore for DevShieldKeystore {
    fn roll_for_next_slot(&self) -> TraitResult<()> {
        Ok(())
    }

    fn next_enc_key(&self) -> TraitResult<Vec<u8>> {
        Ok(self.enc_key_bytes.clone())
    }

    fn current_dec_key(&self) -> TraitResult<Vec<u8>> {
        self.inner.current_dec_key()
    }
}
