---
title: "Hashed receiving address review"
description: "Cross-boundary review of receiving, registration, authorization, recovery and EVM integration, with activation blockers and validation limits."
---

# Hashed receiving address review

Reviewed on October 8, 2026 against `main` for the local continuation of
PR #3220 on `codex/hashed-accounts`. This covers the hashed-account protocol
already on the branch and the new receiving-address integration. Unrelated
fee, dependency and website work in the shared checkout is not included in
the review verdict.

Create a hashed wallet, register it, and wait for finalization before publicly
sharing its `bth1_` receiving address. Wallet creation does not register it.
The original mnemonic and hashed derivation type recover the same
identity on another machine; the current generation comes from the chain.

**The security review is not green.** It found a registration denial of service
that must be resolved before activation. Production remains disabled, weights
remain unmeasured, and the reveal-to-finality cryptographic attack remains
outside this change. Passing functional tests does not remove those limits.

## Boundaries and findings

| Boundary | Behavior reviewed | Result |
| --- | --- | --- |
| Native key derivation → wallet backup | Independent, versioned generation derivation from the original secret; immutable initial descriptor; public-only imports cannot sign | Recovery requires no growing key history. Competing devices still need to coordinate their one pending generation. |
| Native codec → Python bindings → CLI | One canonical 104-character token containing full genesis, descriptor and checksum; no signing public key | Independent fixed vector and malformed-input tests cover the codec. Unknown versions, damaged tokens and wrong networks fail before composition. |
| Address book/wallet object → SDK intent | Complete receiving metadata survives contacts, public keyfiles, defaults, display and copying to a new device | Recipient private keys are not read. The internal SS58 account is selected only at the chain boundary. |
| SDK intent → runtime registration/payment | First-use registration or a check-only guard for an existing recipient, before payment | Runtime tests establish rollback on failed first payment. Concurrent sponsors and a rotation between their payments preserve authority, nonce and provider state. Existing-recipient checks fail after a reorg without reserving funds. |
| Fee/policy → first payment/send-all | Setup reserve, full batch fee and existential deposit are accounted for; reserve stays permanently locked on the first sponsor | Spend caps include the maximum reserve. Send-all computes its transferable balance after setup. Frozen-balance previews are conservative and can reject some otherwise affordable payments. |
| Batch/proxy/multisig → runtime origin | New registration is restricted to authenticated direct calls or a supported flat two-call batch | Unsupported first-use wrappers fail before signing or roll back on chain. Existing guards are flattened. Imported multisig bytes cannot bypass typed, explicit, locally known or implicit recipient setup. |
| Authorization proof → transaction extensions | Account, scheme, generation, next commitment, call and transaction implication are signed together | Tests cover stale/competing generations, tampering, different genesis, invalid nonce/fee/weight rollback, accepted dispatch failure and simulated reverted inclusion. |
| Legacy/EVM/proxy authority → registered identity | Legacy native and Ethereum authorizations cannot spend a registered identity; protected aliases bind to the complete account | Alternate authority paths are checked. Current EVM aliases and all ownership adapters must remain enabled permanently after deployment. |
| H160 lookup → native transfer | Protected aliases resolve through a consistent registry snapshot and keep a registration guard | Native funding to an absent alias is rejected when the feature is enabled because a lookup cannot guard a concurrent mapping change. EVM-side transfers remain the supported route for those H160 recipients. |
| Read-only views and completion callbacks | Typed inputs become network-checked internal accounts for balance/stake reads and subnet completion | Client/snapshot, proxy owner, batch and multisig preview tests cover the normalization boundary. Custom EVM RPCs do not imply a verified native chain. |
| Existing hotkey ownership → first registration | A classical owner assigned before registration causes the registration hook to reject the account | **Unresolved MEDIUM registration denial of service; activation blocker.** |

## Activation-blocking ownership issue

An attacker can see a receiving address, derive its public AccountId and call
the existing hotkey-association operation before its first payment. That
operation does not require the hotkey's consent. It can assign a classical
coldkey as owner. `OnHashedRegistered` correctly refuses to preserve that
classical authority over the new protected identity, but the legitimate
recipient consequently cannot activate the advertised address.

The regression test
`preassociated_classical_owner_blocks_activation_without_funding_the_account`
demonstrates that registration and funding roll back. It proves payment safety
in this case, not account availability. No theft was established by this
finding. Deleting the Owner entry or weakening the guard is not a safe fix:
existing stake and ownership relationships need an explicit consent or migration
rule. The owner-association protocol must be resolved before activation.

## Issues corrected during review

- Added check-only recipient guards after successful registry reads, so
  reorgs cannot silently turn a guarded payment into an unregistered transfer.
- Included first-time registration reserves in spend limits. Existing-recipient
  checks need no reserve and remain usable through restricted proxies.
- Preserved setup metadata in batch children, wallet objects, public-only
  imports and default in-memory hotkeys. Rejected imported multisig bytes when
  their semantic description cannot establish correct recipient setup.
- Normalized typed accounts in batch/multisig previews, proxy origins and
  subnet-registration completion, while retaining the shared receiving address
  for display.
- Corrected EVM alias funding and claim behavior, blocked the absent-alias
  registration race, and removed recommendations to fund an unverified legacy
  mirror. Read-only EVM views validate their native-chain mapping.

## Validation and its limits

The native receiving codec has five passing focused tests, including an
independent BLAKE2/base64 vector, rejection cases and descriptor/account binding.
The focused runtime suite has 22 passing tests. The Python unit-suite run
completed with **2,002 passed and one skipped**. Subsequent small integration
edits were checked with focused SDK/multisig and CLI/EVM suites. Existing wallet
lifecycle tests cover restoring the mnemonic on a different device, dropped
submissions, generation conflicts and reorg resynchronization.

Python wallet tests use real native key derivation and proof verification with
a controlled transport harness. SDK intent tests record composed calls;
they do not simulate FRAME state transitions. Runtime tests independently
exercise FRAME dispatch and storage rollback with explicit test weights.
They do not establish consensus finality, real network fee measurements or a
funded localnet deployment.

Validation commands include:

```bash
cargo test -p bittensor-core keys::receiving --lib --locked
SKIP_WASM_BUILD=1 cargo test -p node-subtensor-runtime hashed_tests --lib --locked
sdk/python/.venv/bin/pytest sdk/python/tests/unit -q
sdk/python/.venv/bin/pytest sdk/python/tests/unit/test_hashed_receiving.py sdk/python/tests/unit/test_multisig_safety.py -q
```

Ruff checks, formatting, generated intent coverage, error-name coverage and
generated documentation checks were also run. Generated documentation was
updated with its generator. The full repository preflight, enabled-chain funded
integration flows, benchmark measurements and deployment review are still
required. This review does not authorize a spec-version bump or production
activation.

## Compatibility limits

The sender needs a wallet that understands the new address format. An older
SS58-only wallet cannot infer the descriptor. Recovery must select the hashed
type and retain any derivation passphrase. Raw calls, some wrapped first-use
operations, external signers, miner HTTP authentication and other off-chain
protocols need explicit support and must not silently reuse rotating spending
keys. The initial profile still uses sr25519; hashing and rotating it does not
make a revealed key post-quantum secure.
