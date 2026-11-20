---
title: EIP-2537 conformance vectors
description: Pinned Ethereum client fixtures for the final BLS12-381 precompile specification.
---

The JSON files are copied without modification from go-ethereum commit
`dcd176767dc3bc9723b9c8abf86491f2fc1225db`, directory
[`core/vm/testdata/precompiles`](https://github.com/ethereum/go-ethereum/tree/dcd176767dc3bc9723b9c8abf86491f2fc1225db/core/vm/testdata/precompiles).
They cover the seven operations in the final EIP-2537 specification. Success
tests check output bytes and gas; failure tests check exceptional failure,
without requiring Geth's implementation-specific error messages.

The upstream go-ethereum library is licensed under LGPL-3.0 (see its
[`COPYING.LESSER`](https://github.com/ethereum/go-ethereum/blob/dcd176767dc3bc9723b9c8abf86491f2fc1225db/COPYING.LESSER)).
These fixtures are test data only and are not included in the runtime binary.
