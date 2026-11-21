# Hickory DNS allocation backport

These are the published `hickory-proto` **0.24.4** and **0.25.2** crates,
patched at the same package versions for the libp2p and litep2p resolvers.
They are excluded from the root workspace and selected through
`[patch.crates-io]`; their dependency versions and features are unchanged.

## Provenance and local changes

- Upstream fix: hickory-dns commit
  [`280c0e5c4e3002d73ba0762835ef82272d1d33a6`](https://github.com/hickory-dns/hickory-dns/commit/280c0e5c4e3002d73ba0762835ef82272d1d33a6),
  released in 0.26.2.
- Advisory: [RUSTSEC-2026-0491](https://rustsec.org/advisories/RUSTSEC-2026-0491.html).
- In both versions, `src/op/message.rs` caps the initial query and record
  vector allocations at eight entries with `Vec::with_capacity(Ord::min(count, 8))`.
  The vectors still grow as records are decoded, so valid messages retain
  their behavior while forged header counts cannot cause multi-megabyte
  allocations before the first record is read.
- `tests/allocation_limit.rs` is added to each crate. It sends a header with
  `u16::MAX` query/answer/authority/additional counts through the real decoder
  and measures the live allocation peak in a separate test process. Each
  standalone `Cargo.toml` retains the published dependency requirements and
  adds only that test target so the test can run without upstream's private
  workspace-only test-support crate.

The audit workflow runs both allocation tests and retains the version-based
`RUSTSEC-2026-0491` ignore because Cargo still identifies these as 0.24.4 and
0.25.2. Reassess that ignore when the SDK networking stack moves to a fixed
upstream Hickory release. This backport does not make the separate DNS codec,
DNSSEC, or zone-parser advisories unreachable; their individual audit
rationales and dependency/feature checks remain in force.
