# Runtime upgrade deploy scripts

Scripts for deploying subtensor runtime upgrades and for signing them as a
member of the sudo multisig (the "triumvirate").

## Script inventory

| Script | Purpose |
| ------ | ------- |
| `deploy-wasm.js` | Direct sudo upgrade for chains where CI holds the sudo key (devnet, testnet, local mainnet clones). Used by `release-train.yml`. |
| `propose-upgrade-multisig.js` | CI's half of the mainnet deployment multisig proposal. Used by `release-train.yml`. |
| `approve-upgrade-multisig.js` | Library for triumvirate first-signer approvals; not run directly. |
| `prod-approval.js` | Runs `approve-upgrade-multisig` with the production triumvirate signatories. |
| `samvps-approval.js` | Same, for the sam-vps multisig. |
| `test-wasm.py` | Verifies a WASM's hash and that its bytes are embedded in the call data. |

## Background: the two multisig layers

A production runtime upgrade is gated by two nested multisigs:

1. **Deployment multisig (2-of-2):** the CI key
   (`5FW3gUUAnWZFTG3QWijcGV9ji3iySsuCj12TQi2eDtgGvxej`) **+** the sudo multisig.
   It holds a `SudoUncheckedSetCode` proxy on the chain's sudo key. CI submits
   the first of the two approvals automatically when the **Release Train**
   GitHub Action's `propose-mainnet` job runs (after devnet and testnet
   checks pass and the `mainnet` environment is approved).
2. **Sudo multisig (2-of-3 triumvirate):** members A, B, C. This is the chain's
   `sudo.key()`. It is the *second* party of the deployment multisig, so the
   triumvirate has to produce that second approval among themselves.

The first triumvirate signer runs `prod-approval.js`. That script builds the
deployment multisig's finalizing call **and** submits the first triumvirate
approval of it in one transaction. The remaining triumvirate signers then
approve (and the last one executes) from PolkadotJS Apps — no script needed.

Once the final approval lands and the upgrade executes on chain, the release
watcher (`watch-mainnet-release.yml`) cuts the GitHub release and publishes the
artifacts.

> You may have been a *second* signer before. That part is done in the Polkadot
> UI. Being the **first** signer is different: it is the script run documented
> below, because only the first signer has to assemble the call data.

## First signer: step by step

Only the **first** signer runs the script. Everyone else approves in the UI.

### 0. One-time setup

```
cd .github/scripts/deploy
npm ci
```

You also need `python3` (for the verification step) and your triumvirate
mnemonic.

### 1. Find the CI proposal run

Open the **Release Train** GitHub Action run that proposed this upgrade
(Actions tab). You need its artifact and one timepoint from it.

### 2. Build the runtime yourself with srtool

Do not sign a WASM you were simply handed. Build the runtime from source with
srtool and confirm your output matches CI's — this is the step that makes the
later check meaningful. srtool builds are deterministic, so an identical source
at the same toolchain produces a byte-identical, identically-hashed runtime.

Check out this repo at the exact ref being deployed (the commit on `main`
that triggered the run) and build with the **same parameters CI uses**. The
authoritative recipe is the srtool build step in
[`.github/workflows/release-train.yml`](../../workflows/release-train.yml);
the key parameters are package `node-subtensor-runtime`, profile `production`,
and build option `--features=metadata-hash`.

```
git checkout <commit-being-deployed>

# srtool expects the runtime crate in a directory named after the package:
ln -s . runtime/node-subtensor

docker run --rm --user root --platform=linux/amd64 \
  -e PACKAGE=node-subtensor-runtime \
  -e BUILD_OPTS="--features=metadata-hash" \
  -e PROFILE=production \
  -v "$(pwd)":/build \
  paritytech/srtool:<rustc-tag> \
  /srtool/build --app
```

Use the srtool image whose Rust version matches subtensor's pinned toolchain
(CI pins this in `scripts/srtool/build-srtool-image.sh`, currently `1.89.0`).
If no prebuilt `paritytech/srtool:<rustc>` image exists for that
version, build one from source with `scripts/srtool/build-srtool-image.sh` —
that is what CI does. The [`srtool-cli`](https://github.com/chevdor/srtool-cli)
wrapper auto-selects the image and is an easier alternative to the raw
`docker run`.

When the build finishes, srtool prints the path to the generated
`...node_subtensor_runtime.compact.compressed.wasm` and a one-line JSON digest
as its final output line.

### 3. Confirm your build matches CI, then assemble the files

Download CI's artifact from the proposal run (Actions → the run →
**Artifacts** → `mainnet-upgrade-<spec_version>`). It contains:

- `subtensor.wasm` and `subtensor-digest.json` — CI's build and digest
- `proxy_proxy_blob.hex` — the call data you will sign
- `pending-release.json` — machine-readable proposal record (timepoint etc.)

Compare your local srtool output against CI's — the SHA256 must be identical:

```
# your local build:
shasum -a 256 .../node_subtensor_runtime.compact.compressed.wasm
# CI's digest (sha256 field, minus the 0x):
cat subtensor-digest.json
```

If they differ, **stop** — your source/toolchain does not match what CI built,
and you must resolve that before signing.

Once they match, put these three files in this directory
(`.github/scripts/deploy`, next to `package.json`). Use **your locally built**
runtime as `subtensor.wasm` and your srtool digest as `subtensor-digest.json`,
plus CI's `proxy_proxy_blob.hex`:

- `subtensor.wasm` (your build)
- `subtensor-digest.json` (your build)
- `proxy_proxy_blob.hex` (from CI)

Then verify the call data embeds that exact runtime:

```
python3 test-wasm.py
```

This confirms `subtensor.wasm` hashes to `subtensor-digest.json` and that those
exact WASM bytes appear inside `proxy_proxy_blob.hex`. It must print
`WASM is correct` before you sign anything. Because the `subtensor.wasm` here is
the one *you* built, a passing check proves the call data you are about to
approve contains the runtime you compiled from source.

### 4. Note the deployment timepoint

Read `blockHeight` and `extrinsicIndex` from CI's `pending-release.json`
(under `proposal`). Those two numbers are the timepoint of CI's
deployment-multisig approval — you pass them to the script in the next step as
`<block>` and `<index>`. The same numbers appear in the **propose** job's
summary table (Block Height / Extrinsic Index).

### 5. Run the first-signer approval

```
npm run prod-approval -- <wss-endpoint> <CI-address> 0 <block> <index> proxy_proxy_blob.hex
```

- `<wss-endpoint>` — the target chain, e.g. `wss://entrypoint-finney.opentensor.ai:443`
- `<CI-address>` — `5FW3gUUAnWZFTG3QWijcGV9ji3iySsuCj12TQi2eDtgGvxej`
- `0` — your signer number; the first signer is always `0`
- `<block>` `<index>` — the timepoint from step 4
- `proxy_proxy_blob.hex` — the CI call-data file from step 3

You will be prompted for your `mnemonic:` (input is hidden). The script first
checks that the triumvirate 2-of-3 derives the on-chain sudo key and aborts if
it does not, then submits your approval.

### 6. Hand off to the other signers

The script writes `deployment-multisig-proposal.hex` and prints a summary
containing a **Call Hash** and the **Block Height / Extrinsic Index** of *your*
approval. Send the other triumvirate signers:

- the **Call Hash**,
- the **timepoint** (your Block Height + Extrinsic Index), and
- the `deployment-multisig-proposal.hex` file (the final signer needs the call
  data).

They then complete it from PolkadotJS Apps (Developer → Multisig, or via the
sudo multisig account) — interior signers `approveAsMulti` with the call hash +
your timepoint, and the last signer `asMulti` with the call data + your
timepoint. When the final approval lands, the spec version bumps and the
release watcher takes over.

## Argument reference

`prod-approval.js` (and `samvps-approval.js`) take:

| pos | first signer (`0`)         | later signers (`n>0`)        |
| --- | -------------------------- | ---------------------------- |
| 2   | wss endpoint               | wss endpoint                 |
| 3   | CI address                 | CI address                   |
| 4   | signer number (`0`)        | signer number                |
| 5   | deploy block height        | deploy block height          |
| 6   | deploy extrinsic index     | deploy extrinsic index       |
| 7   | path to CI call-data hex   | first-approval timepoint height |
| 8   | —                          | first-approval timepoint index  |
| 9   | —                          | path to CI call-data hex     |

The mnemonic is always read interactively, never passed as an argument.

Triumvirate members and the threshold are defined in `prod-approval.js`
(`SUDO_SIGNATORIES`, `SUDO_THRESHOLD`). `samvps-approval.js` is the equivalent
for the sam-vps multisig.
