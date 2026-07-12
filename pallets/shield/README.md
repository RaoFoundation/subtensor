# pallet-shield

FRAME pallet for opt-in, per-block ephemeral-key encrypted transactions (MEV shielding).

## Overview

Block authors rotate ML-KEM-768 key pairs every slot via a mandatory inherent. Users encrypt their extrinsics to the next block author's encapsulation key, preventing front-running and sandwich attacks.

### Key rotation

Each block includes an `announce_next_key` inherent that rotates the key pipeline in four steps:

1. `CurrentKey <- PendingKey` (proposer's decryption target).
2. `PendingKey <- NextKey` (staged one block ahead).
3. `NextKey <- AuthorKeys[next_next_author]` (user-facing, N+2 author's key).
4. `AuthorKeys[current_author] <- announced key` (updated after rotations for consistent reads).

This gives users a full 12-24s submission window (two block periods) instead of 0-12s, eliminating block-boundary timing issues.

### Key expiration

`PendingKeyExpiresAt` and `NextKeyExpiresAt` expose the block number at which each user-facing key stops being valid (exclusive upper bound). Clients can read these directly to know how long a key remains usable.

### Encrypted transaction flow

1. User reads `NextKey` from storage (ML-KEM-768 encapsulation key, 1184 bytes).
2. User encrypts a signed extrinsic with ML-KEM-768 + XChaCha20-Poly1305, producing:

   ```
   ciphertext = key_hash(16) || kem_len(2) || kem_ct || nonce(24) || aead_ct
   ```

3. User submits `submit_encrypted(ciphertext)` signed with their account, using a short mortal era (<=8 blocks).
4. The block author decrypts and includes the inner extrinsic in the same block.

### Transaction extension

`CheckShieldedTxValidity` rejects malformed ciphertext (unparseable structure) at pool validation time. Key hash matching is handled by the block proposer, not the extension.

### Mortal era enforcement

`CheckMortality` (in the runtime) wraps Substrate's `CheckMortality` and rejects `submit_encrypted` calls with immortal or >8-block eras. This ensures stale encrypted transactions are evicted from the pool within a few blocks.

## Storage

| Item | Description |
|------|-------------|
| `CurrentKey` | Current block author's encapsulation key (internal, not for encryption) |
| `PendingKey` | N+1 block author's key, staged before promoting to `CurrentKey` |
| `NextKey` | N+2 block author's key (user-facing, encrypt to this) |
| `AuthorKeys` | Per-authority latest announced encapsulation key |
| `PendingKeyExpiresAt` | Block number at which `PendingKey` is no longer valid |
| `NextKeyExpiresAt` | Block number at which `NextKey` is no longer valid |

## Dependencies

- [`stp-shield`](https://github.com/RaoFoundation/polkadot-sdk) — shared types (`ShieldedTransaction`, `ShieldEncKey`, `InherentType`)
- `ml-kem` / `chacha20poly1305` — cryptographic primitives for in-WASM decryption
