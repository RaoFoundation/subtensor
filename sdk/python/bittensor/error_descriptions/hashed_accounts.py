"""Registered-account authorization diagnostics for fixed and rotating keys."""

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
        "The next commitment violates the account mode. Hashed accounts require a new nonzero "
        "commitment; standard ML-DSA accounts must retain the current commitment. "
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
}
