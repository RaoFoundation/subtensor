---
title: "Pinned mainnet snapshot and synthetic upgrade rehearsal"
description: "Complete logged acquisition proof and exact ten-block verification, with artifact identities and release boundaries."
---

The unchanged repository evidence checker passed. The [full verified result](logged-full-verification.json) proves an upgrade from source runtime version 479 to candidate 480, all upgrade checks and repeated migration idempotency checks, followed by exactly ten contiguous successful **synthetic empty blocks**, 9,298,129 through 9,298,138. Each generated block reports state-root checks and signature checks enabled, `try-state All`, and a successful entire-runtime-state decode. The pinned CLI [help](fast-forward-help.txt.gz) explicitly describes the block count as empty blocks. This snapshot-mode rehearsal does **not** replay archived canonical mainnet transaction bodies or establish that their signatures were validated.

The finalized source was mainnet block **9,298,128**, hash `0x39bc186855bfbf78b4ab9f44138094ea3b19fa64a079161bf617397830aadc1c`, queried from `wss://archive.dev.opentensor.ai:443`. The acquisition and both execution logs report root `0xe6a92abe52fb94bfe734d795e997025c5a390b20160ee69696a1bc6b5058fb06`, exactly matching the separately RPC-pinned header. The [source record](source.json) preserves the genesis hash, spec version and query time.

The first acquisition did not enable remote-ext logging. Its narrower [post-verification record](post-verification.json) and [note](post-verification.md) are retained as historical evidence of that initial limitation. To close that gap, the same pinned block was reacquired with the same pinned CLI and the same archive URI repeated four times, without pallet/prefix filters, using `RUST_LOG=info,remote-ext=debug`. The original files were preserved. The separately acquired snapshot has different serialized bytes, so the same unchanged strict upgrade runner was executed again against that exact snapshot in `logged-verification/`. The resulting logs passed the unchanged full evidence checker. Matching state roots establish the same pinned state; the two snapshot files are not claimed byte-identical.

The completed [acquisition log](snapshot-logged.log.gz) proves **3,304,807 unique top-level keys**, **56 child tries**, and **926,483,590 serialized snapshot bytes**. The [acquisition manifest](logged-acquisition-manifest.json) preserves the exact command, environment, hashes and comparison decision. The checker was not modified, its required fields were not fabricated, and its output is separate from the raw runner result.

| Artifact | SHA-256 |
| --- | --- |
| Original mainnet snapshot | `b24f2b048fa7717d642de4a1227841ff2cd526972043a5ed854aed4a8884e10c` |
| Logged acquisition snapshot | `39f958bc6e86ab896b5fbdcee259709fc1cc3d9a2b220a3ee4d4c56e7b4abcd3` |
| Mainnet try-runtime candidate Wasm | `8be6cd4735812399becf3572f4c13041a55a32052d27cdd526af67b67756af19` |
| Pinned try-runtime 0.10.1 CLI | `d6fcd586fba4c7245668f4ae2a06ffc94e6da4135fae22cfe24453dd11aee3c3` |
| Source JSON | `ad55a6bdc0a668a0b77857afeb38f848aaca5b4c22f024888f759b6aab3e866a` |
| Live pending-probe localnet Wasm (separate artifact) | `4ff98b40b7a38d636bbb1f857103ff3881d6ebb758d030dcb206ec541a10931b` |

These are distinct validation artifact classes. Test-runtime fixtures and localnet validation configuration do not establish production reference weights. The snapshot and compiled binaries are deliberately excluded from this repository evidence package; their hashes, byte lengths and acquisition commands remain recorded.

The upgrade command used `--checks=all`, `--blocktime 12000`, `--no-weight-warnings`, and `--disable-mbm-checks`. Weight warnings were excluded under the user's explicit reference-weight scope exclusion. Multiblock-migration checks were disabled because this runtime configures `MultiBlockMigrator = ()`; this is an explicit scope boundary, not proof of a multiblock migration. The fast-forward command used `--run-migrations --n-blocks 10 --blocktime 12000 --try-state all`. Spec-name/version checks and repeated migration idempotency checks were retained. Exact commands appear in the raw and verified results.

This is read-only acquisition and local runtime execution: no mainnet transaction was submitted. It does **not** remove the hostile pre-registration hotkey-ownership funds-safety blocker, the pending-RPC admission availability gap, or the need for an explicit policy decision if the master switch is expected to revoke cached spending authority. See the [runtime review](../runtime-review.md). Passing this rehearsal is not a declaration of release readiness.
