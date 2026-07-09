# bittensor-drand

Drand timelock encryption for Bittensor commit-reveal weights and general-purpose commitments. Rust core with Python bindings via PyO3.

## What's new in 2.0

- **`get_encrypted_commit` is replaced by `get_encrypted_commit_v2`**. The old modulo-based epoch math (`(block + netuid + 1) / (tempo + 1)`) is gone. The chain now uses a stateful epoch counter (`SubnetEpochIndex`), owner-triggered pending epochs, and configurable tempo. The new function simulates the chain's `block_step` pipeline to predict the exact reveal block.
- **Breaking change**: `get_encrypted_commit_v2` requires epoch schedule state instead of `tempo + current_block + netuid`. The SDK (`bittensor>=11.0.0`) provides this via `subtensor.get_epoch_schedule_state(netuid)`.
- Added `encrypt_mlkem768` and `mlkem_kdf_id` for ML-KEM-768 + XChaCha20Poly1305 encryption.

## Quick start

```python
from bittensor_drand import get_encrypted_commit_v2
```

## Installation

```bash
pip install bittensor-drand
```

For development (build from source):

```bash
git clone https://github.com/opentensor/bittensor-drand.git
cd bittensor-drand
python3 -m venv venv
. venv/bin/activate
pip install maturin bittensor
maturin develop
```

## Usage: commit-reveal weights

### Using the SDK (recommended)

The Bittensor SDK handles all the epoch state plumbing internally. You just call `set_weights`:

```python
import bittensor as bt

sub = bt.Subtensor("local")  # or "finney" for mainnet
wallet = bt.Wallet()

result, message = sub.set_weights(
    wallet=wallet,
    netuid=1,
    uids=[0, 1, 2],
    weights=[0.5, 0.3, 0.2],
    wait_for_inclusion=True,
    wait_for_finalization=True,
)
print(f"Success: {result}, message: {message}")
```

### Using `bittensor_drand` directly

If you need lower-level control (custom clients, miners, tooling):

```python
import bittensor as bt
from bittensor_drand import get_encrypted_commit_v2

sub = bt.Subtensor("local")
netuid = 1
current_block = sub.get_current_block()

# Fetch epoch schedule state from chain
schedule = sub.get_epoch_schedule_state(netuid, block=current_block)
reveal_period = sub.get_subnet_reveal_period_epochs(netuid=netuid)

uids = [1, 3]
weights = [100, 200]  # u16 values after convert_weights_and_uids_for_emit
version_key = 843000
wallet = bt.Wallet()

commit_bytes, reveal_round = get_encrypted_commit_v2(
    uids=uids,
    weights=weights,
    version_key=version_key,
    last_epoch_block=schedule.last_epoch_block,
    pending_epoch_at=schedule.pending_epoch_at,
    subnet_epoch_index=schedule.subnet_epoch_index,
    tempo=schedule.tempo,
    blocks_since_last_step=schedule.blocks_since_last_step,
    current_block=current_block,
    subnet_reveal_period_epochs=reveal_period,
    block_time=12.0,
    hotkey=wallet.hotkey.public_key,
)

print(f"Encrypted commit: {len(commit_bytes)} bytes, reveal round: {reveal_round}")
```

## General-purpose encryption

### Encrypt a string for N blocks into the future

```python
from bittensor_drand import get_encrypted_commitment

encrypted, reveal_round = get_encrypted_commitment(
    "my secret data",
    blocks_until_reveal=10,
    block_time=12.0,
)
print(f"Encrypted: {len(encrypted)} bytes, reveal at round {reveal_round}")
```

### Encrypt / decrypt binary data (round-trip)

```python
from bittensor_drand import encrypt, decrypt

encrypted, reveal_round = encrypt(b"binary payload", n_blocks=5, block_time=12.0)

# Later, after the reveal round has passed:
decrypted = decrypt(encrypted, no_errors=False)
```

### Encrypt for a specific Drand round

```python
from bittensor_drand import encrypt_at_round

encrypted, reveal_round = encrypt_at_round(b"payload", reveal_round=17200000)
```

### Batch decryption with a pre-fetched signature

When decrypting multiple ciphertexts for the same round, fetch the signature once:

```python
from bittensor_drand import decrypt_with_signature, get_signature_for_round

sig = get_signature_for_round(reveal_round=17200000)
plaintext = decrypt_with_signature(encrypted, sig)
```

## ML-KEM-768 encryption

For post-quantum key encapsulation (used by the chain's `NextKey` rotation):

```python
from bittensor_drand import encrypt_mlkem768, mlkem_kdf_id

# pk_bytes: 1184-byte ML-KEM-768 public key from NextKey storage
blob = encrypt_mlkem768(pk_bytes, b"plaintext", include_key_hash=True)

# Blob format (include_key_hash=True):
#   [key_hash(16)][u16 kem_len LE][kem_ct][nonce24][aead_ct]

kdf = mlkem_kdf_id()  # b"v1" — raw shared secret, no HKDF
```

## Testing on a local subnet

1. Start a local subtensor node with configurable tempo support:

```bash
LOCALNET_IMAGE_NAME=ghcr.io/opentensor/subtensor-localnet:pr-2638 ./scripts/localnet.sh
```

2. Create a subnet and configure hyperparameters:
    - Set `commit_reveal_weights_enabled` to `True`
    - Set `tempo` to your desired value (e.g. `360`)
    - Set `weights_rate_limit` to `0` (for faster testing)

3. Register a wallet to the subnet.

4. Run a weight-setting script:

```python
import bittensor as bt
from bittensor import logging

logging.set_info()

sub = bt.Subtensor("local")
wallet = bt.Wallet()

result, message = sub.set_weights(
    wallet=wallet,
    netuid=1,
    uids=[0],
    weights=[1.0],
    wait_for_inclusion=True,
    wait_for_finalization=True,
)
logging.info(f"Result: {result}, message: {message}")
```

5. Wait for the reveal epoch, then verify weights were applied:

```python
import bittensor as bt

sub = bt.Subtensor("local")
print(sub.weights(netuid=1))
```

## API reference

| Function | Description |
|---|---|
| `get_encrypted_commit_v2(...)` | Encrypt weights for commit-reveal using stateful epoch model |
| `get_encrypted_commitment(data, blocks, block_time)` | Timelock-encrypt a string |
| `encrypt(data, n_blocks, block_time)` | Timelock-encrypt binary data |
| `encrypt_at_round(data, reveal_round)` | Encrypt for a specific Drand round |
| `decrypt(data, no_errors=True)` | Decrypt (auto-fetches Drand signature) |
| `decrypt_with_signature(data, sig_hex)` | Decrypt with a pre-fetched signature |
| `get_signature_for_round(round)` | Fetch Drand BLS signature for a round |
| `get_latest_round()` | Get the latest Drand round number |
| `encrypt_mlkem768(pk, plaintext, hash)` | ML-KEM-768 + XChaCha20Poly1305 encryption |
| `mlkem_kdf_id()` | Returns KDF identifier (`b"v1"`) |

## Build & test

```bash
pip install maturin
maturin develop
cargo test
pytest tests/ -v
```
