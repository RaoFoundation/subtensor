---
title: "Subtensor FIPS 204 verifier provenance"
description: "Pinned upstream source and consensus-specific verification changes."
---

# Subtensor verifier provenance

This directory vendors the published `fips204` 0.4.6 source from
`https://github.com/integritychain/fips204`, upstream commit
`8d456d989dce681233ef963d3c3306da8f76d7c7`. Original Apache-2.0 and MIT licenses
are retained. The manifest enables only ML-DSA-65 by default and has no OS RNG
feature enabled. Existing dependency versions are retained where possible.

All vendored Rust source is normalized by this checkout's `cargo fmt` policy,
including otherwise unchanged upstream modules. This formatting-only churn is
separate from the cryptographic modifications below; original filenames and
licenses are preserved. Review semantic changes against the pinned upstream
commit with whitespace differences ignored. Trailing whitespace in the upstream
README and changelog is also removed to satisfy the repository diff check.

Local cryptographic changes:

- Verification uses bounded rejection sampling: at most 894 XOF output bytes
  per matrix polynomial and 221 per complete challenge sampler, including its
  initial eight bytes. The bounded sampler implementations follow the upstream
  algorithms. Failure returns false and polynomial buffers zeroize on drop.
- The verification arithmetic is shared by ordinary verification and a new
  `PublicKey::verify_mu` entry point. This permits streaming the full pure-mode
  transcript in the runtime without a second large allocation. Callers must
  reconstruct the message representative from the actual key, context, and
  message; an untrusted supplied digest is not authentication.
- Signing and key generation are unchanged upstream algorithms and are intended
  for SDK/test callers, not on-chain execution. Unused signing code is removed
  from the runtime by normal dead-code elimination.

The native account primitives add byte-format and external pure-mode tests.
NIST fixtures are a recorded ML-DSA-65 subset of ACVP revision
`65370b861b96efd30dfe0daae607bde26a78a5c8`, as distributed by upstream 0.4.6.
