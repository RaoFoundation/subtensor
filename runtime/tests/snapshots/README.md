---
title: "Fee and gas baseline approval"
description: "Explicit human authorization required for changes to accepted runtime fee and gas snapshots."
---

These snapshots are accepted fee and gas expectations for
[`fee_baseline.rs`](../fee_baseline.rs). They must not be refreshed merely to make
CI or a local test pass.

Changing, adding, deleting, or renaming a baseline requires **explicit and specific
permission from a human for that baseline change**. Present the affected calls
and files, old and proposed new values, and the reason for the change before
requesting permission. Approval must cover those baselines and value changes.
Record that approval and its exact scope in the commit or PR.

A request to fix CI/tests, update weights, prepare a release, or commit/push does
not authorize changing baselines. Neither an automated review nor generated
benchmark output supplies human approval. Do not loosen tolerances or change
comparators to accept a regression in place of obtaining approval.

For diagnosis and verification, run:

```sh
INSTA_UPDATE=no cargo test -p node-subtensor-runtime --test fee_baseline
```

Do not run `INSTA_UPDATE=always`, `force`, or `unseen`, `cargo insta accept`, or
equivalent baseline replacement commands before approval. Only after approval,
scope regeneration to the approved test cases and include only approved snapshot
changes. A generated `.snap.new` file is a proposed result, not permission to
replace the accepted `.snap` file or commit the proposal.
