"""Hashed account registration and rotating authorization diagnostics."""

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
        "The rotation proof uses a different generation from HashedAccounts.Accounts. "
        "Wait for pending transactions, refresh chain state, and derive the current key again."
    ),
    "WrongCommitment": (
        "The revealed signing key does not match the account's current commitment. "
        "Check the mnemonic, derivation version, and current generation before signing again."
    ),
    "InvalidNextCommitment": (
        "The next signing-key commitment is invalid or repeats the current commitment. "
        "Derive the next generation independently using the wallet's versioned derivation."
    ),
    "GenerationExhausted": (
        "The hashed account has reached the maximum supported generation and cannot rotate again. "
        "Do not wrap or reset the counter; further authorization requires a protocol upgrade."
    ),
    "AliasCollision": (
        "The derived EVM alias is already bound to another account. This descriptor cannot be "
        "registered safely; create a new hashed wallet and use its new descriptor."
    ),
}
