# Root parameters as code

These files describe root-controlled, chain-wide configuration. They do not change runtime code. Editing a file creates a proposal; it does not apply a change by itself.

- `devnet.toml`, `testnet.toml`, `mainnet.toml`: desired values, initially exported from one finalized live block per network.
- `export.js`: reads all supported storage at one finalized block and writes fresh TOML.
- `diff.js`: reads live storage, separates reviewed edits from unrelated drift, and writes JSON/Markdown proposals.
- `lib/params.js`: the explicit storage-to-root-setter allowlist.
- `networks.json`: public endpoints used for reads.

The reporting tools use Node.js 22.5+ (CI uses 24) and `@polkadot/api`. They have no signing key, wallet, seed, or submission path. They encode calls and calculate hashes only.

## Units and values

Balances are raw rao, not TAO. Fixed-point values use raw integer bits, for example:

```toml
[subnet_moving_alpha]
setter = "AdminUtils.sudo_set_subnet_moving_alpha"
value = { bits = "1288490" }
```

Do not substitute a decimal fraction for raw bits. Integers are quoted so 64-bit and 128-bit values do not lose precision. Enum variants are strings. An absent optional account is represented as `{ none = true }` because TOML has no null value.

Each table names its exact allowed setter. The loader rejects missing, unknown, or substituted tables and setters. The `basket_claim_dust` table is one atomic four-argument call, so changing any member writes all four members shown in the file. Every member is displayed and old-value checked. A grouped edit mixed with unrelated member drift is rejected rather than silently writing the drifted member.

## Scope

The files cover 51 storage values: 47 single-value setters plus the four values in the basket-claim-dust group. Covered categories include delegate and childkey takes and rate limits; network registration, lock, immunity, and dissolve settings; EVM chain ID; commit-reveal version; CK burn; admin freeze and owner hyperparameter limits; flow and emission-gate settings; mechanism count; start-call and coldkey-swap delays; basket trading and claim-dust settings; hashed-account settings; epoch scheduling; moving alpha; nomination requirements; emission bars; and the TAO/USD price oracle.

Excluded by design:

- netuid-keyed and subnet-owner-settable hyperparameters, including tempo, adjustment intervals, registration targets, kappa/rho, and per-subnet weight rate limits;
- global defaults that have no corresponding root-only setter;
- `sudo_toggle_evm_precompile`, which is keyed by precompile rather than being one chain-wide scalar;
- `sudo_set_basket_trading_frozen`, which manages an account-keyed kill-switch set;
- operational root calls that do not represent a configuration value.

Review side effects, not only storage writes. The nomination minimum setter can scan every alpha stake entry and clear small nominations in one extrinsic. The current runtime compares the previous effective minimum with the new raw factor, so do not assume a decrease in raw factor skips the scan. Changing an emission bar clears its stored gate bar for recomputation. Changing the oracle clears the cached TAO/USD price. Basket claim dust is atomic.

## Refresh and compare

Install the isolated, locked dependency graph and run the network-free tests:

```bash
npm ci --ignore-scripts --prefix sudo
npm test --prefix sudo
```

Export live values. This overwrites the selected files in the output directory, so use a temporary directory when reviewing rather than importing a new baseline:

```bash
node sudo/export.js all --output-dir /tmp/sudo-export
node sudo/export.js mainnet --output-dir /tmp/sudo-export
```

Every readable value is recorded. Any endpoint failure, missing storage item, or decode failure is reported and produces a nonzero exit. A partial export keeps an `UNREAD LIVE VALUES` comment; it is not a valid complete desired-state file.

Compare a reviewed edit with the live chain and its PR base:

```bash
git show "$(git merge-base HEAD origin/main):sudo/mainnet.toml" > /tmp/mainnet-base.toml
node sudo/diff.js mainnet \
  --file sudo/mainnet.toml \
  --base-file /tmp/mainnet-base.toml \
  --output-dir /tmp/sudo-mainnet-proposal
```

Values already equal to live state are skipped. A value edited from its base is proposed. A value that still equals its base but differs on chain is **unrelated live drift** and is excluded from the call. An exact reviewed baseline is required for call data: without `--base-file`, every live/file mismatch is reported as unclassified drift and the output contains no call data. CI uses the PR merge base, the commit before a main push, or a required manual-dispatch baseline ref. Historical files can omit newly tracked settings and retain retired settings; new settings count as edits, and retired settings never generate calls. An initial export without a historical file is report-only. The proposal records source/baseline commits and file SHA-256 hashes.

Every changed value becomes its allowlisted setter in `Utility.batch_all`, wrapped in `Sudo.sudo`. The tool resolves calls and argument types from live metadata and SCALE round-trips each setter and the complete outer call before writing call data.

## CI proposals

`.github/workflows/sudo-params.yml` runs for pull requests touching `sudo/**`, pushes to main touching it, and manual dispatches. A manual dispatch must name a reviewed baseline ref to distinguish unrelated drift. It runs offline tests, then checks every managed parameter against live metadata and performs read-only comparisons for all three networks. PR and non-main runs are previews and omit approval commands. Artifacts upload before the optional Discord notification. JSON and Markdown artifacts include the finalized-block context, source/baseline provenance, decoded old-to-new changes, unrelated drift, encoded call data, and call hash. Every proposal is written to the workflow step summary.

Devnet and testnet CI simply produce the decoded diff, decoded call, call data, and call hash for review. There is no additional apply-status section. CI never signs or submits an extrinsic.

### Discord proposal notifications

After generating a proposal, CI sends **one Discord message per network with at least one proposed change**. Zero-change and drift-only reports do not notify. The message lists every `parameter: old -> new` change, a compact decoded call, the call hash, the merged PR (or a commit-link fallback), the run containing the countersign command, and a UTC timestamp. Mentions are disabled, and long decoded calls are truncated with a pointer to the run URL.

Notifications run only for pushes to `main` after merge and manual dispatches on `main`. Pull-request runs never receive the webhook or post messages, avoiding secret exposure to untrusted changes and notification noise on each PR update. `pull_request_target` is not used. The optional repository secret `SUDO_DISCORD_WEBHOOK` is passed only to the trusted notification step as an environment variable. If it is unset or empty, CI emits a warning, skips the notification, and succeeds; the webhook is never logged. GitHub's read-only token resolves the merged PR through the commit-to-PR API.

The message is informational only. It does not execute a call, apply a proposal, watch for execution, or schedule network scans.

## Mainnet triumvirate review

A changed proposal from a trusted main-branch run includes an approval template. CI never uses a wallet or signs. The configured 2-of-3 legacy signer set must derive the live `Sudo.Key`. Do not sign from a PR preview or run code shipped in an artifact.

On your trusted signer machine, use the updated repository SDK and repository verifier. Independently reproduce the proposal from its listed source and baseline commits and hashes. Compare the old-to-new values, decoded call, SCALE data and BLAKE2-256 hash. Run:

```bash
SUDO_WALLET=your-wallet python3 sudo/countersign.py downloaded.json independently-reproduced.json \
  --expected-call-hash 0xYOUR_REVIEWED_HASH --operation open
```

Only the designated initiator uses `open`, once. Share its opening `HEIGHT:INDEX` with the remaining signers. They run the same verifier with their own wallet and that exact timepoint instead of `open`. The approval cannot open a fresh operation if the reviewed timepoint was executed, canceled or replaced. The verifier checks both artifacts, the allowlisted storage mapping and fresh finalized old values after confirmation. The updated `btcli` independently checks the composed hash before signing; its pinned multisig approval carries that exact timepoint to the chain.

**Known limitation:** old-value checks are not atomic with multisig execution. If an emergency update lands after verification but before execution, the runtime does not enforce a compare-and-set. Stop on any observed drift, regenerate, and restart review. Do not reuse an old opening command, even if the values have since returned to their old state. The `*-args.json` file has exact JSON number tokens for inspection or SDK composition; it is not an alternative approval path.

The runtime-upgrade `SudoUncheckedSetCode` proxy cannot apply arbitrary parameter calls.

## Emergency drift recovery

1. Use the established private sudo/multisig process to make the urgent on-chain change first. Do not wait for a file PR to protect the chain.
2. Export the resulting live value into a temporary directory and review its decoded storage and side effects.
3. Commit the new value to the appropriate network file in a follow-up PR, recording the finalized block and reason. The PR base comparison will classify the already-applied edit as a reviewed file change while skipping its now-correct setter.

This ordering handles an emergency without teaching CI to revert unrelated out-of-band changes.
