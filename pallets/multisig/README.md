---
title: "Multisig pallet"
description: "Pinned FRAME multisig implementation with optional off-curve composite accounts."
---

# Multisig Module
A module for doing multisig dispatch.

Forked from `RaoFoundation/polkadot-sdk` commit
`cacb4310f20c7cac83eb3ccd8ed5a5ad4212608a`, package `pallet-multisig` 41.0.0.
Call indices 0–4, legacy derivations, storage, and events retain their original format.
Indices 5–9 use the shared bounded off-curve derivation without identity registration.
Ordinary multisig deposits and delegation policy still apply.
The separate safe derivation weight must remain disabled at `Weight::MAX` until
reference measurements establish its bound.

- [`Config`](https://docs.rs/pallet-multisig/latest/pallet_multisig/pallet/trait.Config.html)
- [`Call`](https://docs.rs/pallet-multisig/latest/pallet_multisig/pallet/enum.Call.html)

## Overview

This module contains functionality for multi-signature dispatch, a (potentially) stateful
operation, allowing multiple signed
origins (accounts) to coordinate and dispatch a call from a well-known origin, derivable
deterministically from the set of account IDs and the threshold number of accounts from the
set that must approve it. In the case that the threshold is just one then this is a stateless
operation. This is useful for multisig wallets where cryptographic threshold signatures are
not available or desired.

## Interface

### Dispatchable Functions

- `as_multi` - Approve and if possible dispatch a call from a composite origin formed from a
  number of signed origins.
- `approve_as_multi` - Approve a call from a composite origin.
- `cancel_as_multi` - Cancel a call from a composite origin.

[`Call`]: ./enum.Call.html
[`Config`]: ./trait.Config.html

License: Apache-2.0
