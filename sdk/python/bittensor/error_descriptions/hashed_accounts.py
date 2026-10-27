"""Registered-account authorization diagnostics for rotating keys."""

DESCRIPTIONS: dict[str, str] = {
    "Disabled": (
        "Hashed accounts are not enabled on this chain. Check AdminUtils.HashedAccountsEnabled; "
        "creating a wallet locally does not activate runtime support."
    ),
    "UnsupportedDescriptor": (
        "The hashed descriptor version or underlying signing scheme is unsupported. "
        "Use the descriptor exported by a wallet version supported by this runtime."
    ),
    "DescriptorMismatch": (
        "The descriptor does not match the account's permanent authorization record. "
        "Check the destination and exported public descriptor; re-registration cannot reset keys."
    ),
    "NotRegistered": (
        "The hashed account has no authorization record. A sponsor must register its public "
        "descriptor before funds or account roles are assigned to it."
    ),
    "WrongGeneration": (
        "The authorization proof uses a different sequence from HashedAccounts.Accounts. "
        "Wait for pending transactions and refresh chain state before signing again."
    ),
    "WrongCommitment": (
        "The revealed signing key does not match the account's current commitment. "
        "Check the mnemonic, derivation version, and current generation before signing again."
    ),
    "InvalidNextCommitment": (
        "Hashed accounts require a new nonzero "
        "commitment different from the current key. "
        "Restore the wallet with its original signing scheme and account mode."
    ),
    "GenerationExhausted": (
        "The account has reached the maximum authorization sequence and cannot sign again. "
        "Do not wrap or reset the counter; further authorization requires a protocol upgrade."
    ),
    "AliasCollision": (
        "The derived EVM alias is already bound to another account. This descriptor cannot be "
        "registered safely; create a new hashed wallet and use its new descriptor."
    ),
    "AuthenticationKindConflict": (
        "The account already has a different permanent authorization kind. Direct and derived "
        "authorities cannot replace one another; check the registered account and descriptor."
    ),
    "InvalidDerivedAuthority": (
        "The composite descriptor, derived account ID, or approving origin is invalid. "
        "Multisig protection must be approved by the composite's actual quorum."
    ),
    "DerivedAuthorityTooComplex": (
        "The derived authority exceeds the runtime's member, depth, or resolution bounds. "
        "Use a smaller supported authority structure."
    ),
    "CapabilityMissing": (
        "No active capability authorizes this owner's purpose key. Install the intended "
        "Order or Message capability through the owner's authority before signing."
    ),
    "CapabilityInvalid": (
        "The capability key, scope, expiry, or proof is invalid. Check the full capability "
        "record and ensure the signing key matches its registered commitment."
    ),
    "CapabilityExpired": (
        "The capability's block-height expiry has passed. The owner must install a new grant "
        "and the purpose key must sign with its new epoch."
    ),
    "CapabilityEpochMismatch": (
        "The capability epoch is stale or does not match the owner's permanent epoch counter. "
        "Refresh finalized capability state; revocation permanently invalidates old proofs."
    ),
    "CapabilityScopeViolation": (
        "The operation exceeds the installed capability's identities, fee, audience, per-order "
        "amount, or total spending limits. Obtain an appropriate owner-approved grant."
    ),
}
