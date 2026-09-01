# bittensor

A lean Python SDK and CLI for the Bittensor chain. One install gives you both a
library (`import bittensor`) and a command line (`btcli`).

**Documentation: [bittensor.com/docs](https://bittensor.com/docs)**

This package supersedes the separate
[`bittensor-cli`](https://pypi.org/project/bittensor-cli/) and
[`bittensor-wallet`](https://pypi.org/project/bittensor-wallet/) packages:
`btcli` and the wallet ship here.

The design goal is a thin, unopinionated wrapper: easy, safe, and fast to do the
core chain operations, with nothing hidden. It does **not** implement the neuron
networking layer (axon/dendrite/synapse) — it is the layer that talks to the
chain.

## Why it's built this way

Almost everything is a projection of the chain's own runtime metadata:

- The **generated layer** (`bittensor.storage`, `runtime_api`, `constants`,
  `calls`) is emitted from metadata, so it can't drift from the chain.
- **Reads** (81 of them) and **intents** (72 of them) are the small hand-written
  semantic layer on top — the part metadata can't express (units, safety,
  aggregation).
- The **CLI is generated from those registries**: every read becomes a
  `btcli query <name>`, every intent becomes a `btcli tx <name>`. Adding
  an SDK operation adds its command for free, and the two can't diverge.
- A CI gate proves every chain call is either wrapped by an intent or explicitly
  marked raw-only, so nothing is silently forgotten.

## Install

Requires Python 3.10–3.14. Using [uv](https://docs.astral.sh/uv/):

```bash
uv venv && source .venv/bin/activate
uv pip install -e .
```

This installs the `btcli` command and the `bittensor` Python package. The
CLI is a first-class part of the package: its terminal-UI dependencies
(typer, rich) and EVM signing (eth-account) are always installed.

## CLI

### Networks and configuration

Every command accepts the connection and identity options. Set them per-command,
via environment variables, or persist them once:

```bash
btcli config set network test
btcli config set wallet my_coldkey
btcli config get            # show the whole config
```

Precedence, highest first: **CLI flag > environment variable > config file >
built-in default**. So after the config above, `btcli query tx-rate-limit`
talks to testnet, but `btcli -n finney query tx-rate-limit` overrides it for
that one call.

Global options appear on every command's `--help`:

| Option | Env | Meaning |
| --- | --- | --- |
| `--network`, `-n` | `BT_NETWORK` | `finney` / `test` / `local`, or a `ws://` endpoint |
| `--wallet`, `-w` | `BT_WALLET` | coldkey wallet name |
| `--wallet-hotkey`, `-H` | `BT_WALLET_HOTKEY` | hotkey name within the wallet |
| `--wallet-path` | `BT_WALLET_PATH` | wallet directory |
| `--json` | | machine-readable JSON output |
| `--yes`, `-y` | | skip confirmation prompts |
| `--dry-run` | | preview a mutation without submitting |
| `--quiet`, `-q` | | suppress informational output |

### Shell completion

Ready-made completion scripts ship in `completions/` (and are installed into the
standard system locations — `share/bash-completion/completions`,
`share/zsh/site-functions`, `share/fish/vendor_completions.d`). On most system or
Homebrew-style installs shells pick them up automatically, so there's nothing to
run.

If your shell doesn't auto-load them (e.g. a venv/`uv` install), source the
script once from your rc file:

```bash
# bash (~/.bashrc)
source /path/to/completions/btcli.bash
# zsh (~/.zshrc) — or drop `_btcli` on your $fpath
source /path/to/completions/_btcli
# fish — copy into a dir fish auto-loads
cp /path/to/completions/btcli.fish ~/.config/fish/completions/
```

`btcli --install-completion` still works if you'd rather have it edit your
rc file for you.

### Wallets

```bash
btcli wallet create -w my_coldkey
btcli wallet list
btcli wallet regen-coldkey -w my_coldkey     # prompts for the mnemonic securely
btcli wallet sign --message "hello" --use-hotkey -w my_coldkey
btcli wallet verify --message "hello" --signature 0x... --ss58 5F...
```

### Reading state

```bash
btcli wallet balance 5F...coldkey
btcli subnets list
btcli subnets show 1
btcli stake show --hotkey 5F...validator --netuid 1

# Generated read commands (one per SDK read):
btcli query metagraph --netuid 1
btcli query delegate-take --hotkey 5F...
btcli query crowdloan --crowdloan-id 0
btcli query --help          # full list
```

### Submitting transactions

Every intent is a `btcli tx <name>`; run `btcli tx --help` for the list.

```bash
# Preview first (fee, effects, policy) — nothing is submitted:
btcli tx add-stake --hotkey 5F...validator --netuid 1 --amount-tao 10 --dry-run

# Submit (prompts to confirm unless --yes):
btcli tx add-stake --hotkey 5F...validator --netuid 1 --amount-tao 10 -w my_coldkey
btcli tx transfer --dest 5F...dest --amount-tao 1.5 -w my_coldkey --yes
```

### Address arguments accept local names

Any address option (`--dest`, `--hotkey`, `--coldkey`, and the `wallet balance` address) takes three forms:

- a raw ss58 address;
- a **local key name** — hotkey options take `HOTKEY` or `WALLET/HOTKEY`; coldkey
  options take a wallet name;
- **omitted**, in which case `--hotkey` / `--coldkey` fall back to your
  configured wallet's key. Destination-style options never default.

```bash
btcli query hotkey-owner --hotkey my_coldkey/my_hotkey
btcli wallet balance my_coldkey    # resolves the wallet's coldkey
```

## SDK

### Reading

`bt.Subtensor` is one class for both worlds: blocking when used directly,
async when awaited. Blocking mode connects lazily and needs no `close()`.

```python
import bittensor as bt

sub = bt.Subtensor()          # defaults to finney; "test" | "local" | "ws://..."

# Typed namespaces — every read in the catalog, one namespace per
# category, with autocomplete and typed returns
bal = sub.balances.get("5F...coldkey")
subnets = sub.subnets.all()
neurons = sub.neurons.all(netuid=1)
cost = sub.subnets.subnet_registration_cost()
take = sub.delegation.delegate_take(hotkey_ss58="5F...")

# The same reads dispatched by name (the form agents and `btcli query` use)
take = sub.read("delegate_take", hotkey_ss58="5F...")

# Generic accessors over the generated descriptors — anything on chain
tempo = sub.query(bt.storage.SubtensorModule.Tempo, [1])
ed = sub.constant(bt.constants.Balances.ExistentialDeposit)
```

Async is the same surface awaited:

```python
import asyncio
import bittensor as bt

async def main():
    async with bt.Subtensor() as client:   # or: client = await bt.Subtensor()
        bal = await client.balances.get("5F...coldkey")

asyncio.run(main())
```

### Writing: intents, plan, execute

A mutation is an **intent** — a serializable dataclass. `plan` previews it
(fee, effects, warnings, policy) without submitting; `execute` signs and submits
through a single policy-gated choke point.

```python
from bittensor.wallet import Wallet
wallet = Wallet(name="my_coldkey", hotkey="my_hotkey")

async with bt.Subtensor() as client:
    intent = bt.AddStake(hotkey_ss58="5F...validator", netuid=1, amount_tao=10)

    plan = await client.plan(intent, wallet)
    print(plan.fee, plan.effects, plan.ok)

    result = await client.execute(intent, wallet)
    print(result.success, result.block_hash)
```

Build intents by name (handy for agents and tools):

```python
await client.execute_tool("transfer", {"dest_ss58": "5F...", "amount_tao": 1.0}, wallet)
```

### Safety: Policy

Attach a `Policy` to bound what any mutation may do; violations raise
`PolicyError` at execute time (and show up in `plan`).

```python
policy = bt.Policy(max_spend_tao=5.0, allowed_netuids=[1, 2])
async with bt.Subtensor("finney", policy=policy) as client:
    ...
```

### Typed money

`Balance` is unit-tagged (TAO vs. a subnet's alpha) and refuses to mix units, so
you can't accidentally pass alpha where TAO is expected. Construction and
readback are unit-named: `from_tao` / `.tao` for TAO, `from_alpha` / `.alpha`
for a subnet's alpha (`.tao` on an alpha balance raises). Use strings/`Decimal`
for exact large amounts.

```python
bt.Balance.from_tao("10000000.123456789")   # exact TAO
bt.Balance.from_alpha(2.5, netuid=42)       # subnet-42 alpha, prints with the
                                             # subnet's on-chain token symbol
                                             # (α₄₂ before a client connects)
bt.tao(1.5); bt.alpha(2.5, 42); bt.rao(1_500_000_000)
```

Alpha is never summed across subnets or silently treated as TAO. To value stake
in TAO, use the `stake_value_for_coldkey` read — a spot-price mark
(`alpha × price`, excludes slippage/fees), pinned to one block.

### Typed errors

Failures come back as `ExtrinsicResult` with a machine-readable `ErrorCode` and a
remediation hint, derived from the exact chain error name:

```python
result = await client.execute(bt.BurnedRegister(netuid=999), wallet)
if not result.success:
    print(result.error.code, result.error.remediation)  # e.g. subnet_not_exists
```

## Advanced submission modes

These compose with any intent:

- **Proxy** — sign as a registered proxy so the real coldkey stays offline:

  ```python
  await client.execute(intent, delegate_wallet, proxy_for="5F...real_coldkey")
  ```
  On the CLI: `--proxy-for <ss58|wallet>` on every mutation command. Manage
  delegations with the `add-proxy` / `remove-proxy` intents and the `proxies` read.

  A zero-delay `Validate` proxy needs no subnet code changes. The signing
  hotkey is direct and every other target must have granted it a `Validate`
  proxy. Targets supplied by subnet code through the client constructor are
  merged with `WEIGHT_TARGETS`, so validator operators can add targets without
  changing the subnet. Duplicates are removed while constructor order is
  preserved; an empty merged set disables weight submission entirely.

  ```console
  WEIGHT_TARGETS=5F...DELEGATE,5F...VALIDATOR_A,5F...VALIDATOR_B
  ```

  The existing subnet call remains unchanged:

  ```python
  result = await client.execute(
      bt.SetWeights(netuid=1, weights={0: 0.2, 1: 0.8}), delegate_wallet
  )
  ```

  Targets are dispatched with `Utility.force_batch`: one revoked or invalid
  target is reported in `result.data["weight_results"]` without preventing the
  remaining validators from setting weights.

- **Atomic batch** — several intents in one all-or-nothing extrinsic:

  ```python
  await client.execute(bt.Batch(intents=[
      {"op": "transfer", "dest_ss58": "5F...", "amount_tao": 1.0},
      {"op": "add_stake", "hotkey_ss58": "5F...", "netuid": 1, "amount_tao": 2.0},
  ]), wallet)
  ```

- **MEV-shielded** — encrypt the call so the mempool can't front-run it (a
  mainnet feature; needs validator-side reveal):

  ```python
  await client.submit_shielded(bt.Transfer(dest_ss58="5F...", amount_tao=1.0), wallet)
  await client.submit_shielded(intent, delegate_wallet, proxy_for="5F...real_coldkey")
  ```

  Shield wraps the composed call (proxy and saved-multisig included). Vault,
  Ledger, and the browser extension each sign twice: the inner call, then the
  carrier.

  The CLI shields stake-trading commands (`stake add/remove/move/swap/transfer`,
  `unstake-all`, and the limit variants) by default. Opt out per command with
  `--no-mev-shield`, persistently with `btcli config set mev_shield false`, or
  opt any other mutation in with `--mev-shield`.

## EVM

Subtensor runs a full EVM; its accounts (h160, MetaMask-style) and native
accounts (ss58) are disjoint signing domains on the same chain. `btcli evm`
owns the seam — keys, address math, money movement, association, and
precompiles — so none of it needs JS scripts or MetaMask.

**Full walkthrough:** [EVM guide](/docs/guides/evm) (quick start, diagrams,
command reference, precompiles).

```bash
btcli evm key new -w my_coldkey            # encrypted keystore V3, next to your hotkeys
btcli evm fund --amount-tao 5              # coldkey -> EVM key (via its ss58 mirror)
btcli evm balance                          # in TAO and wei (EVM uses 18 decimals, not 9)
btcli evm send --to 0x... --amount-tao 1        # h160 -> h160
btcli evm send-to-ss58 --to my_coldkey --amount-tao 1  # stored EVM key -> ss58 (precompile)
btcli evm claim-deposit --amount-tao 1          # MetaMask deposit -> coldkey (substrate extrinsic)
btcli evm associate --netuid 1                  # link the EVM key to your hotkey, proof included
btcli evm stake add --netuid 1 --hotkey 5F... --amount-tao 2   # stake from the EVM key

btcli evm call metagraph getUidCount 1     # any precompile; view calls are free, no key
btcli evm precompiles                      # the catalog: names, addresses, descriptions
btcli evm abi staking-v2                   # address + ABI JSON for Hardhat/ethers/viem
btcli evm config --format metamask         # ready-to-paste tool config (also: hardhat, remix)
btcli evm doctor                           # probe RPC, chain ID, gas price, key balance
```

Keystore files import directly into MetaMask/geth/ethers (`btcli evm key
export`), and `--evm-key` accepts stored key names everywhere. In the SDK the
same layer is `bittensor.evm`: `h160_to_ss58` (the mirror mapping, verified
against the chain's own `address-mapping` precompile), `ss58_to_pubkey` (the
`bytes32` form precompiles take), keystore management, the precompile catalog
with call encoding, and wei/TAO conversions. Substrate-side EVM intents
(`fund_evm_key`, `evm_withdraw` via `claim-deposit` or `tx evm-withdraw`,
`associate_evm_key`) ride the normal
plan/execute/policy flow.

## Escape hatch: raw calls

Every chain call the metadata exposes is available under `bittensor.calls`, even
the ones no intent wraps. Submit one directly (an active `Policy` refuses this
unless it sets `allow_raw_calls=True`):

```python
call = bt.calls.Commitments.set_commitment(netuid=1, info={...})
await client.submit_call(call, wallet, signer="hotkey")
```

## For agents

The full catalog of executable operations and their JSON schemas is available
programmatically and on the CLI, so an agent can discover and call everything
without hard-coded knowledge:

```bash
btcli tools        # machine-readable JSON of every intent + params
```

```python
bt.intents.list_tools()
```

Combined with `--json` on every command, `--dry-run` to preview, and
`Policy` to bound spend/netuids, the CLI is safe for automated use (it refuses to
hang on a prompt: a non-interactive session without `--yes` is declined, not
blocked).

## What's covered

The intent layer wraps every user-facing state transition across the runtime —
staking, transfers, registration, weights, children/take, proxies, multisig,
crowdloans, coldkey swap, identity, leasing, auto-staking, serving, and
subnet-owner hyperparameters. Calls left as raw-only are deprecated, root/admin
only, or off-chain-signed; each is recorded with a reason and reachable through
`submit_call`.

## Development

Dev environment and gates run through [uv](https://docs.astral.sh/uv/) and
[just](https://github.com/casey/just):

```bash
just sync         # install the locked dev environment
just check        # lint + typecheck + unit tests + codegen gates (same as CI)
just test         # offline unit tests only
just fmt          # auto-fix lint findings and reformat
```

The generated layer is emitted from a node's metadata and committed. Regenerate
and check it against a running node:

```bash
python -m codegen <ws-endpoint>          # regenerate bittensor/_generated
python -m codegen.check --drift <ws>     # committed files match chain metadata
python -m codegen.check --coverage       # every chain call has a deliberate status
python -m codegen.check --names          # every classified error name still exists
```

The migrated chain-facing SDK coverage lives in the Rust core e2e suite:

```bash
E2E_ENDPOINT=ws://127.0.0.1:9944 cargo test -p bittensor-core --test e2e -- --nocapture
```
