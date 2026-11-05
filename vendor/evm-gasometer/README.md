---
title: "Local EVM gasometer patch"
description: "Pinned upstream gasometer with an early account-read hook for SELFDESTRUCT."
---

# EVM gasometer 0.43.0

Vendored from the crates.io `evm-gasometer` 0.43.0 package (Apache-2.0),
checksum `54aa0327b242bad8dd83ba524effd1d798e9102ff592910dfdded79c6bde4ff3`.
The package version and dependency requirements are unchanged.

The sole behavior change calls `ExternalOperation::IsEmpty` before
SELFDESTRUCT reads its source balance during dynamic gas calculation.
The existing hook conservatively covers that account read. Frontier charges
stateful address mapping costs at this hook before storage access, including
when ordinary opcode gas calculation subsequently fails. The remaining opcode
surcharge excludes the reads already paid here.

Keep this patch until the pinned upstream dependency exposes an equivalent
pre-read hook. Regression tests exercise the actual stack executor in Frontier.
