from typing import Union, Optional

from bittensor_drand.bittensor_drand import (
    get_encrypted_commit_v2 as _get_encrypted_commit_v2,
    get_encrypted_commitment as _get_encrypted_commitment,
    encrypt as _encrypt,
    encrypt_at_round as _encrypt_at_round,
    decrypt as _decrypt,
    decrypt_with_signature as _decrypt_with_signature,
    get_signature_for_round as _get_signature_for_round,
    get_latest_round as _get_latest_round,
    encrypt_mlkem768 as _encrypt_mlkem768,
    mlkem_kdf_id as _mlkem_kdf_id,
)


def get_encrypted_commit_v2(
    uids: list[int],
    weights: list[int],
    version_key: int,
    last_epoch_block: int,
    pending_epoch_at: int,
    subnet_epoch_index: int,
    tempo: int,
    blocks_since_last_step: int,
    current_block: int,
    subnet_reveal_period_epochs: int,
    block_time: Union[int, float],
    hotkey: bytes,
) -> tuple[bytes, int]:
    """Returns encrypted commit and target round using the stateful epoch model (v2).

    Arguments:
        uids: The uids to commit.
        weights: The weights associated with the uids.
        version_key: The version key to use for committing and revealing.
        last_epoch_block: Block at which the last epoch ran for this subnet.
        pending_epoch_at: Pending owner-triggered epoch block (0 if none).
        subnet_epoch_index: Monotonic epoch counter for the subnet.
        tempo: Epoch duration in blocks.
        blocks_since_last_step: Blocks since the last step for the subnet.
        current_block: Chain head block number.
        subnet_reveal_period_epochs: Number of epochs before reveal.
        block_time: Amount of time in seconds for one block.
        hotkey: Committer hotkey public key bytes (wallet.hotkey.public_key).

    Returns:
        commit (bytes): Encrypted and compressed uids & weights payload.
        target_round (int): Drand round number when weights can be revealed.

    Raises:
        ValueError: If the input parameters are invalid or encryption fails.
    """
    return _get_encrypted_commit_v2(
        uids,
        weights,
        version_key,
        last_epoch_block,
        pending_epoch_at,
        subnet_epoch_index,
        tempo,
        blocks_since_last_step,
        current_block,
        subnet_reveal_period_epochs,
        block_time,
        hotkey,
    )


def get_encrypted_commitment(
    data: str, blocks_until_reveal: int, block_time: Union[int, float] = 12.0
) -> tuple[bytes, int]:
    """Encrypts arbitrary string data with time-lock encryption.

    Arguments:
        data: The string data to encrypt.
        blocks_until_reveal: Number of blocks until the data should be revealed.
        block_time: Amount of time in seconds for one block. Defaults to 12 seconds.

    Returns:
        encrypted_data (bytes): Raw bytes of the encrypted data.
        target_round (int): Drand round number when data can be revealed.

    Raises:
        ValueError: If encryption fails.
    """
    return _get_encrypted_commitment(data, blocks_until_reveal, block_time)


def encrypt(
    data: bytes, n_blocks: int, block_time: Union[int, float] = 12.0
) -> tuple[bytes, int]:
    """Encrypts arbitrary binary data with time-lock encryption.

    Arguments:
        data: The binary data to encrypt.
        n_blocks: Number of blocks until the data should be revealed.
        block_time: Amount of time in seconds for one block. Defaults to 12 seconds.

    Returns:
        encrypted_data (bytes): Raw bytes of the encrypted data.
        target_round (int): Drand round number when data can be revealed.

    Raises:
        ValueError: If encryption fails.
    """
    return _encrypt(data, n_blocks, block_time)


def encrypt_at_round(data: bytes, reveal_round: int) -> tuple[bytes, int]:
    """Encrypts arbitrary binary data for a specific Drand reveal round.

    Arguments:
        data: The binary data to encrypt.
        reveal_round: The specific Drand round number when decryption becomes possible.

    Returns:
        encrypted_data (bytes): Raw bytes of the encrypted data.
        reveal_round (int): The Drand round number when data can be revealed (same as input).

    Raises:
        ValueError: If encryption fails.
    """
    return _encrypt_at_round(data, reveal_round)


def decrypt(encrypted_data: bytes, no_errors: bool = True) -> Optional[bytes]:
    """Decrypts previously encrypted data if the reveal time has been reached.

    Arguments:
        encrypted_data: The encrypted data to decrypt.
        no_errors: If True, returns None instead of raising exceptions when decryption fails.
                  If False, raises exceptions on decryption failures.

    Returns:
        decrypted_data (Optional[bytes]): The decrypted data if successful, None otherwise.

    Raises:
        ValueError: If decryption fails and no_errors is False.
    """
    return _decrypt(encrypted_data, no_errors)


def decrypt_with_signature(encrypted_data: bytes, signature_hex: str) -> bytes:
    """Decrypts data using a provided Drand signature.

    Arguments:
        encrypted_data: The encrypted data to decrypt.
        signature_hex: Hex-encoded Drand BLS signature for the reveal round.

    Returns:
        decrypted_data (bytes): The decrypted data.

    Raises:
        ValueError: If decryption fails or signature is invalid.
    """
    return _decrypt_with_signature(encrypted_data, signature_hex)


def get_signature_for_round(reveal_round: int) -> str:
    """Fetches the Drand signature for a specific round.

    Arguments:
        reveal_round: The Drand round number to fetch the signature for.

    Returns:
        signature_hex (str): Hex-encoded BLS signature for the round.

    Raises:
        ValueError: If the signature cannot be fetched or is not yet available.
    """
    return _get_signature_for_round(reveal_round)


def get_latest_round() -> int:
    """Gets the latest revealed Drand round number.

    Returns:
        round (int): The latest revealed Drand round number.

    Raises:
        ValueError: If fetching the latest round fails.
    """
    return _get_latest_round()


def encrypt_mlkem768(pk_bytes: bytes, plaintext: bytes, include_key_hash: bool = False) -> bytes:
    """Encrypts data using ML-KEM-768 + XChaCha20Poly1305.

    Arguments:
        pk_bytes: ML-KEM-768 public key bytes (from NextKey storage, 1184 bytes)
        plaintext: Data to encrypt.
        include_key_hash: If True, prepends the twox_128 hash of pk_bytes (16 bytes) to the output.

    Returns:
        bytes: Encrypted blob

    Raises:
        ValueError: If encryption fails (invalid public key, buffer too small, etc.)
    """
    return _encrypt_mlkem768(pk_bytes, plaintext, include_key_hash)


def mlkem_kdf_id() -> bytes:
    """Returns the KDF identifier used by ML-KEM encryption.

    Returns:
        bytes: KDF identifier (b"v1")
    """
    return _mlkem_kdf_id()
