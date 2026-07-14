# Runtime upgrade deploy scripts

Scripts for deploying subtensor runtime upgrades and for signing them as a
member of the sudo multisig (the "triumvirate").

## Script inventory

| Script | Purpose |
| ------ | ------- |
| `deploy-wasm.js` | Direct sudo upgrade for chains where CI holds the sudo key (devnet, testnet, local mainnet clones). Used by `release-train.yml`. |
| `propose-upgrade-multisig.js` | CI's half of the mainnet deployment multisig proposal. Used by `release-train.yml`. |
| `approve-upgrade-multisig.js` | Library for triumvirate first-signer approvals (legacy path); not run directly. |
| `prod-approval.js` | Runs `approve-upgrade-multisig` with the production triumvirate signatories (legacy path). |
| `samvps-approval.js` | Same, for the sam-vps multisig. |
| `sudo-signatories.json` | The triumvirate signer set + threshold; consumed by `prod-approval.js` and embedded into `upgrade-manifest.json` by the release train. |
| `test-wasm.py` | Verifies a WASM's hash and that its bytes are embedded in the call data (legacy path; `btcli upgrade check` supersedes it). |

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

When the proposal lands, the release train also publishes a **proposal
pre-release** — `v<spec_version>`, tagged at the exact commit being deployed —
whose page carries the signer instructions and these assets:

- `subtensor.wasm` and `subtensor-digest.json` — CI's deterministic srtool build
- `proxy_proxy_blob.hex` — the call data being signed
- `pending-release.json` — machine-readable proposal record (timepoint etc.)
- `upgrade-manifest.json` — everything btcli needs (call hash, commit, signer
  set, asset URLs)

The **URL of that pre-release page is the one thing signers need**. Once the
final approval lands and the upgrade executes on chain, the release watcher
(`watch-mainnet-release.yml`) promotes the pre-release to the final release
and publishes the rest of the artifacts.

## Signing an upgrade (the btcli path)

Every triumvirate signer — first, interior, or last — runs the same command:

```
btcli upgrade sign --url https://github.com/<org>/subtensor/releases/tag/v<spec> -w <your-wallet>
```

Before submitting anything, btcli verifies:

- the call data is *exactly*
  `proxy.proxy(sudo_key, None, sudo.sudoUncheckedWeight(system.setCode(<wasm>), <pinned weight>))`
  — no batches, no extra calls (re-encoded byte-for-byte against live chain
  metadata);
- the embedded runtime matches the release's srtool digest;
- the proxied account is the chain's live `sudo.key()`;
- a pending on-chain deployment-multisig proposal carries `blake2_256(call data)`;
- the resolved signer set derives the on-chain sudo key.

It then reads your position from chain state — whether the sudo multisig
operation is not yet opened (you are the first signer), underway (interior
approval), or one approval short (your `as_multi` executes the upgrade) — and
submits the matching extrinsic. No signer numbers, no timepoints, no
PolkadotJS.

Related commands:

```
btcli upgrade pending           # discover pending proposals from chain state alone
btcli upgrade check --url ...   # run every verification without signing
btcli multisig pending -w <ms>  # pending sudo-multisig ops; also lists pending upgrades
```

### Verifying against code you built yourself (recommended)

Do not sign a WASM you were simply handed. srtool builds are deterministic:
identical source at the same toolchain produces a byte-identical runtime. The
pre-release tag *is* the code being deployed.

The short version is one command (requires docker). Run it from a checkout
you already trust — reviewed `main`, not the proposal tag, so the proposal
cannot supply the verifier that vouches for it:

```
git fetch origin && git checkout origin/main
./scripts/verify-upgrade.sh
```

It downloads the release manifest and wasm, fetches the proposal commit and
rebuilds it from a pristine clone in the same srtool container CI used,
byte-compares the result against the released runtime, runs
`btcli upgrade check` against the chain if btcli is installed, and prints
the `btcli upgrade sign` command pinned to your own build. It never submits
anything.

The manual recipe, if you prefer to run each step yourself:

```
git fetch origin && git checkout v<spec>

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
(CI pins this in `scripts/srtool/build-srtool-image.sh`; the release's
`upgrade-manifest.json` records the version used). If no prebuilt
`paritytech/srtool:<rustc>` image exists for that version, build one from
source with `scripts/srtool/build-srtool-image.sh` — that is what CI does. The
[`srtool-cli`](https://github.com/chevdor/srtool-cli) wrapper auto-selects the
image and is an easier alternative to the raw `docker run`.

Then pin the call data to *your* build:

```
btcli upgrade check --url <release-url> --wasm ./runtime/target/srtool/production/wbuild/node-subtensor-runtime/node_subtensor_runtime.compact.compressed.wasm
btcli upgrade sign  --url <release-url> --wasm <same path> -w <your-wallet>
```

A passing check then proves the call data executes `setCode` with exactly the
runtime you compiled from source. Without `--wasm`, `check`/`sign` still verify
the published artifacts against each other and the chain, but the runtime bytes
are trusted from the release.

This structure — a URL anyone can fetch, call data anyone can re-derive from
source, and an on-chain hash anyone can compare — is the template for later
decentralized governance: any holder can `btcli upgrade check --url ...` and
assert the proposal on chain matches the code they are looking at.

## Legacy path: the node scripts

The pre-btcli flow still works and is kept as a fallback. It is documented
here in abbreviated form; the scripts are unchanged.

1. **Find the CI proposal run** (Actions → Release Train) or the proposal
   pre-release; download `subtensor.wasm`, `subtensor-digest.json`,
   `proxy_proxy_blob.hex`, and `pending-release.json`.
2. **Build the runtime yourself with srtool** (see above) and confirm your
   sha256 matches CI's digest. If they differ, **stop**.
3. **Verify the call data** — place `subtensor.wasm` (your build),
   `subtensor-digest.json` (your digest), and `proxy_proxy_blob.hex` (CI's) in
   this directory and run `python3 test-wasm.py`. It must print
   `WASM is correct`.
4. **Note the deployment timepoint** — `blockHeight` / `extrinsicIndex` under
   `proposal` in `pending-release.json`.
5. **First signer only** — one-time `npm ci` in this directory, then:

   ```
   npm run prod-approval -- <wss-endpoint> <CI-address> 0 <block> <index> proxy_proxy_blob.hex
   ```

   You are prompted for your mnemonic (hidden). The script checks the
   triumvirate 2-of-3 derives the on-chain sudo key, then submits the first
   approval and writes `deployment-multisig-proposal.hex`.
6. **Hand off** — send the other signers the printed **Call Hash**, the
   **timepoint** of your approval, and `deployment-multisig-proposal.hex`.
   Interior signers `approveAsMulti` (call hash + timepoint) and the last
   signer `asMulti` (call data + timepoint) from PolkadotJS Apps — or they
   simply run `btcli upgrade sign --url ...`, which interoperates with
   approvals made either way.

### Argument reference (legacy scripts)

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

Triumvirate members and the threshold are defined in `sudo-signatories.json`
(read by `prod-approval.js` and embedded into the release manifest).
`samvps-approval.js` is the equivalent for the sam-vps multisig.
