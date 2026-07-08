# py-sp-core

Python bindings (via [PyO3](https://pyo3.rs) + [maturin](https://www.maturin.rs)) for Substrate `sp-core` key primitives, built against the **same `sp-core` revision as the runtime** in this monorepo. It replaces the parts of the external `bittensor-wallet` (btwallet) package that wrap sp-core, so the Python SDK's wallet crypto can never drift from the chain's.

## What it provides

The compiled module is importable as `py_sp_core` and is re-exported by the SDK as `bittensor.sp_core` (`sdk/python/bittensor/sp_core.py`).

- **`Keypair`** — sr25519 / ed25519 keypairs backed by sp-core:
  - construction: `create_from_mnemonic`, `create_from_seed`, `create_from_uri` (e.g. `//Alice`), `create_from_private_key`, `create_from_encrypted_json` (PolkadotJS v3 keystore), or public-only from an SS58 address / raw public key
  - `sign` / `verify` (including btwallet's `<Bytes>...</Bytes>` wrapping fallback)
  - `encrypt` / `decrypt` / `encrypt_for` — ed25519 sealed-box message encryption (libsodium)
  - `generate_mnemonic` (12–24 words)
- **SS58 helpers** — `ss58_encode`, `ss58_decode`, module-level `verify` against an SS58 address (aliases `encode_ss58`, `decode_ss58`, `verify_signature` kept for pre-migration callers)
- **Keyfile compatibility** — read/write the `bittensor-wallet` on-disk keyfile format:
  - `serialized_keypair_to_keyfile_data` / `deserialize_keypair_from_keyfile_data`
  - `encrypt_keyfile_data` / `decrypt_keyfile_data` supporting NaCl (`$NACL`), Ansible Vault (`$ANSIBLE_VAULT`), and legacy Fernet (`gAAAAA`) encryption, plus the `keyfile_data_is_encrypted*` / `keyfile_data_encryption_method` detectors
  - `get_password_from_environment` / `save_password_to_environment`
- **Exceptions & constants** — `KeyfileError`, `WrongPasswordError`, `CRYPTO_SR25519` (1), `CRYPTO_ED25519` (0), matching the py-substrate-interface / btwallet convention

## Usage

```python
import py_sp_core as sp

kp = sp.Keypair.create_from_mnemonic(sp.Keypair.generate_mnemonic(12))
sig = kp.sign(b"hello")
assert kp.verify(b"hello", sig)
assert sp.verify(b"hello", sig, kp.ss58_address)

alice = sp.Keypair.create_from_uri("//Alice")
print(alice.ss58_address)  # 5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY
```

## Building

The crate builds as an abi3 extension module (Python ≥ 3.10) with maturin:

```bash
# develop install into the active venv
maturin develop -m py-sp-core/Cargo.toml

# or build a wheel
maturin build --release -m py-sp-core/Cargo.toml
```

In the monorepo, the Python SDK (`sdk/python`) depends on this crate directly via a `tool.uv.sources` path override, so `uv sync` in `sdk/python` builds it automatically. Published SDK wheels fall back to the PyPI release of the same package.

## Layout

- `src/lib.rs` — `Keypair`, SS58 helpers, signature verification, module definition
- `src/keyfile.rs` — btwallet-compatible keyfile encryption (NaCl / Ansible Vault / legacy Fernet)
- `src/keyfile_codec.rs` — keypair ⇄ keyfile JSON serialization
- `src/encrypted_json.rs` — PolkadotJS encrypted JSON keystore import

## Tests

Rust unit tests live in the crate (`cargo test -p py-sp-core`). Parity and compatibility tests against the old btwallet behavior live in the Python SDK: `sdk/python/tests/unit/test_sp_core_keypair.py`, `test_sp_core_parity.py`, `test_wallet_compat.py`, and `test_keyfiles_golden.py`.
