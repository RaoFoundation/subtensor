import time

import pytest
import bittensor_drand as btcr


def test_get_latest_round():
    round_ = btcr.get_latest_round()
    assert isinstance(round_, int)
    assert round_ > 0


def test_encrypt_and_decrypt():
    data = b"hello, bittensor!"
    n_blocks = 1

    encrypted, reveal_round = btcr.encrypt(data, n_blocks)
    assert isinstance(encrypted, bytes)
    assert isinstance(reveal_round, int)

    print(f"Reveal round: {reveal_round}")
    current_round = btcr.get_latest_round()

    if current_round < reveal_round:
        print("Waiting for reveal round to arrive...")
        while btcr.get_latest_round() < reveal_round:
            time.sleep(3)

    decrypted = btcr.decrypt(encrypted)
    assert decrypted is not None
    assert decrypted == data


def test_encrypt_at_round_and_decrypt():
    data = b"test data for specific round"

    # Get a round that's already revealed (in the past)
    current_round = btcr.get_latest_round()
    past_round = current_round - 100  # Use a round from the past

    # Encrypt at specific round
    encrypted, returned_round = btcr.encrypt_at_round(data, past_round)
    assert isinstance(encrypted, bytes)
    assert returned_round == past_round

    # Should be able to decrypt immediately since the round is in the past
    decrypted = btcr.decrypt(encrypted)
    assert decrypted is not None
    assert decrypted == data

    # Test with future round
    future_round = current_round + 1000
    encrypted_future, returned_future_round = btcr.encrypt_at_round(data, future_round)
    assert isinstance(encrypted_future, bytes)
    assert returned_future_round == future_round

    # Attempting to decrypt future round should fail or return None
    decrypted_future = btcr.decrypt(encrypted_future, no_errors=True)
    assert decrypted_future is None  # Can't decrypt yet


def test_get_signature_for_round():
    # Get a past round that's already revealed
    current_round = btcr.get_latest_round()
    past_round = current_round - 100

    # Fetch signature for that round
    signature = btcr.get_signature_for_round(past_round)
    assert isinstance(signature, str)
    assert len(signature) > 0
    # Drand signatures are hex-encoded, so should only contain hex characters
    assert all(c in "0123456789abcdef" for c in signature.lower())


def test_decrypt_with_signature():
    # Test basic decrypt_with_signature functionality
    data = b"test data for signature decryption"

    # Get a round that's already revealed
    current_round = btcr.get_latest_round()
    past_round = current_round - 100

    # Encrypt at that round
    encrypted, returned_round = btcr.encrypt_at_round(data, past_round)
    assert returned_round == past_round

    # Fetch signature separately
    signature = btcr.get_signature_for_round(past_round)

    # Decrypt using the signature
    decrypted = btcr.decrypt_with_signature(encrypted, signature)
    assert decrypted == data


def test_batch_decryption_optimization():
    """Test the main use case: decrypting multiple ciphertexts with one signature fetch."""
    # Simulate batch encryption for the same round
    messages = [
        b"message 1",
        b"message 2",
        b"message 3",
        b"message 4",
        b"message 5",
    ]

    # Get a past round
    current_round = btcr.get_latest_round()
    past_round = current_round - 100

    # Encrypt all messages at the same round
    encrypted_messages = [btcr.encrypt_at_round(msg, past_round)[0] for msg in messages]

    # Fetch signature once
    signature = btcr.get_signature_for_round(past_round)

    # Decrypt all messages using the same signature (no additional API calls)
    decrypted_messages = [
        btcr.decrypt_with_signature(enc, signature) for enc in encrypted_messages
    ]

    # Verify all messages decrypted correctly
    assert decrypted_messages == messages
    print(
        f"Successfully decrypted {len(messages)} messages with a single signature fetch!"
    )


def test_get_encrypted_commitment():
    encrypted, round_ = btcr.get_encrypted_commitment("my_commitment", 1)
    assert isinstance(encrypted, bytes)
    assert isinstance(round_, int)


def test_get_encrypted_commit_v2():
    # cycle_reset row from src/epoch_schedule_vectors.rs
    uids = [0, 1]
    weights = [100, 200]
    version_key = 1
    encrypted, round_ = btcr.get_encrypted_commit_v2(
        uids,
        weights,
        version_key,
        last_epoch_block=10,
        pending_epoch_at=0,
        subnet_epoch_index=0,
        tempo=50,
        blocks_since_last_step=0,
        current_block=10,
        subnet_reveal_period_epochs=1,
        block_time=12.0,
        hotkey=bytes([1, 2, 3]),
    )
    assert isinstance(encrypted, bytes)
    assert len(encrypted) > 0
    assert isinstance(round_, int)
    assert round_ > 0


# ML-KEM-768 test key (1184 bytes) - valid ML-KEM-768 public key
VALID_MLKEM768_PK = b'>\x82\xb6V\xd4\x840\xd6\x14\x1d\x17\xa7\xc6\xd4D\xab@\x1b\xb3\x04\x9e\xaa\t\x04v\xfb]K\xd2\xbd\x04\xf3\xa8\xe2QW\x99\x80\x9bv\xe4\x86\x9e\x92.\xa8xO\xfe\x84\x9ef\xb9f\xf2\x1b\x158A\x0fC\x19\x84\xcbRF\x89\xd8F\xbf\xc7\x1d\x0b\xa6g_\xa6\xaa\x00:\x9d\x86\x8aQ\xe0`^_\x93\x11\x0b\x91\x1a\x02\x91gz\xec"%h^\xeey\x06\xf0qq\xad\xacD\xb8V\xce4\xdb<\xc5\xa6\r\x0f\xe0\xa5D\xa1F\xcb\xc0f\x96\xf7.\r\xd9\n\xafGt:LS_\xf2\x95<9\x16\x17\xc7\x17\xe7\xac\x08|\xacL^Q\x80\x99h\x08-\xc6\xb8\x14\xc8\x96j\xe7\'\x1e\xc0Y\xa8"\xc9\xef\xec\'\xdd\x8b\xaa\xce\xc79\xd7\xf3\x05%r\x82H*&\xa0\x8a\xba\x94)\xcer\xf3\xb6\x82\x05E\xc04I\x10\xa0\xcd2\xc0.@J@\xb3\xda\x08\x04\x9c\xcd\xb0k\x05\x97\xb6x\x93\x10\x8d/\xb2\xca\\\xf8\x95\x0b;\x1dGX\x08H\xc9w\xec\x1az\xfaj%\x98\xcaI\x04,-=\xeak\x9a\xfa&Ku\xaa\xce\x14C\xe6"^2E\x00\xfbW#\xa4!\xa3\xd7KU\xe774c\x18]H\xa4~UCy\x91:\x9b\xe6\xc6\xa8\xeb\xb2]\xa2\x9c\x8a\xf0\xfby\x96T\x18D".?\x83|\x81\x16$cK! \x90\xadug\x9f$\xd9&>Q\x9d\xffbB\xea\xd6@{|\x0e\rK\xb5\x0eY\x02\xd9\x99\\@\xf1\x12\x81S\x85\xa1<p\x10Ch\xd6\x8b?\x10\xe1\xa1\xe9\xe6\x0b\xd8p\xb0\x1f\xe9wb\xe1\x92"\xc9\xb5\rb\xa1\\\xb9\xcb\x18\xd1j\xcc\xc6\x8e\xac\xdbJ\xc6\x81\x93;\xc5\x15e\x00\tAIg\xf3*\xcezf\xc7\xf9\x92\x93\x81\xd5\x1f\x16\x89\x81\xd3\xe4T>+\x81\xba\x02\xa5\x89\x82L\\\x8b\x1b\x80!\x0e\xcf\xd6jf\xc0\x00\xf6\x83\xb8l|*\x13rn\xd3a\x8c\x9cI\xc6w;r\x954}\xa5\xd1\x0e\xb3\x92|\x05$\x87^\x1b\x02dkc6\xf7\r\xc7H\x99o\x0b9?\xecG\xf0AOO;=\xae\x1b=Z\xf6\x05\x93\xc5\x81\x98\x93\x1be\xf1>\xadr\x1f\xda\x13osl\x178c_\xac\x93\xac\x19Z\xc9a\xc5\x1e\x88#G\xfaaF\xccu\xa3\xae\xa0W8\x99Q\x02\x1b\xc4\xee\\\x87\x1e`_\x8ek\x95\x19\xe5\x925\x16\xc0)R\x0b\x0bqY%png\x17\x9cQ\xb1&\xd7$S&\x13!\xbc\xb7\x9eM\xc9%X\x96\xcc&"\xb7\x1ej\x9c\x13@ao\x97\x00\xf1)C#zXOG\x12\xaf\x04\xa8\xff k\xcc\x99\xae\xcd65\xed\x95#3\xd5\x88\x90 ;\x87\xd3w\x11\xdaIh\x8c<W\xd0<kc\x1c\r\x0b\xbe\xdei\xa2X\xeb\xb3\xb2\xbb\xa3\x13\x89E\x91\xab\xc07\xd9O\x1c9\x81\xc5\xe1\xc8\xecIi\x05\xac\x10;%\x1f\x88K\xb7}\xa6)V\xc0\x94\xf2\xba@\x03WC\x8b\x925\xa7\xa0\x0c\xab\x90\x1e\xf66\xc7k,\xbeL\x82\xcaM\xb1\x85\x05\x03Lc \xcdI\x07/q\x91@\xdac\'\xbc\x01l!\xa1\x1f\x8cb"9\x08\xcd=y\x01H\x9b\xc1\xfc\xe1\x19y\xeb\x04Jk\xa5;S\x9bV\xb1#\xa1\xb8\xba\x8d\xf8\x8a\xb8\xd0{\xf1\x06\x05\xacp\xb8\x1c\'\x96\xce\xd8\xa9\x12\x9c\x8cFS\x07:\n\x04\xdf\x88L\xae,z#r\x00\x92D\x8c\x0b\x83\xc3y\xc6\x8c\xde\xc3;\x83\x92\x8cs\xa0l\xb0\xab_\x88\x84\xb0\xe1U\x8e\xaf"c\x82\xe3\x87W \x17\xa7+\x9e\x1b%=\xc2\x87\xa2\xcf\xd2\xac\xae\xd7\t\x01:-4\xb9\xa0\xabB\xc5\xd13B\x19P)\xa2\xd2Q\xf4\xa1\x04\xb8x=\x86|*\x80\x82 \xfa\xd4P\xf1\xcb\x1d\xe9\x98_b\xd7\x94\xc5\x9c\x92m\x80\x95n\xba\x7f\xdb\x9b\x0f\x06\xbc}(P#w\xa0}\x9d\x01_\x83\x8cWQ\x1c\x82\xc0\xd0\t\xef\xea!\xd5b\x93\x93\'{\xed\xa7J\xd78S\xde\x86|\xa3\x93\xcdk\xf9-\xe5\x9a\xb4XPsz\xc4yT\xe1#\xcbHq87\x0b\xf7\x10V\x05\xac\x03\x0c\xeb\x16\x96\x83H\xd5\xca\x02jaJ.F\xc4h\x84#\x9f\x14z\xd8\x02\x18@i;\xaa<F\xb5\x93l\x88"#\xf0\x9a\xb8\xd7C\\-\x88\x83\xe5\xe4\xb0\xf3\xd8/\xff\x16\x1e|G\xce:CA`\x08\x80\xed\xe8\x885\xa3"\xf2\x80\xb1x\xe3\xaeL\x16\x17\xf8(\x07\xefl\xcf\x9ct\x94n\x92J\x0e#\xbb\xfeI;\xc8\xc2ibA\x8e\xbdh{C\x8c\x8a\xaf)\x85\x04\x1c\x81_4a\'3\x02\xe2lF\x87\x8b\xc8\xcd\xac\xb0\xd9\x90\\X\xb4-@ce@\xf1\x0f&[X\xb1\x1a\x11v\xdb\x9c\xbf\xa3\x9f\x1eq\xee-\xbf:\xa1\x15\x857\xad\xca\xcf\xaa\x8a\x0cI\xab\xf6\x04\x93\xeda5\xbeL\xf9\xda\x9d'


@pytest.mark.parametrize(
    "pk_bytes,should_succeed",
    [
        # Valid key (1184 bytes)
        (VALID_MLKEM768_PK, True),
        # Invalid key (first bytes modified, but correct length)
        (b"\x00\x00\x00\x00" + VALID_MLKEM768_PK[4:], True),
        # Invalid key (wrong length)
        (VALID_MLKEM768_PK[:1000], False),
        # Invalid key (empty)
        (b"", False),
    ],
    ids=["valid", "invalid_first_bytes", "invalid_length", "empty"],
)
def test_encrypt_mlkem768(pk_bytes, should_succeed):
    """Test ML-KEM-768 encryption with correct and incorrect public keys."""
    plaintext = b"test message for ML-KEM-768 encryption"

    if should_succeed:
        # With valid key - should encrypt successfully
        ciphertext = btcr.encrypt_mlkem768(pk_bytes, plaintext)
        assert isinstance(ciphertext, bytes)
        assert len(ciphertext) > 0

        # Verify blob format: [u16 kem_len][kem_ct][nonce24][aead_ct]
        assert len(ciphertext) >= 2 + 24  # At least kem_len (2) + nonce (24)

        # Check format: first 2 bytes - KEM ciphertext length (little-endian)
        kem_len = int.from_bytes(ciphertext[0:2], byteorder="little")
        assert kem_len > 0
        assert kem_len <= 1500  # Reasonable maximum for ML-KEM-768

        # Check that nonce (24 bytes) follows kem_ct
        nonce_start = 2 + kem_len
        assert len(ciphertext) >= nonce_start + 24

        # Check that AEAD ciphertext follows nonce
        aead_start = nonce_start + 24
        assert len(ciphertext) >= aead_start + len(plaintext)  # AEAD adds overhead

        # Verify that each call creates unique ciphertext (due to random nonce)
        ciphertext2 = btcr.encrypt_mlkem768(pk_bytes, plaintext)
        assert ciphertext != ciphertext2, (
            "Ciphertexts should differ due to random nonce"
        )
    else:
        # With invalid key - should raise ValueError
        with pytest.raises(
            ValueError, match="Failed to decode public key|Failed to decode"
        ):
            btcr.encrypt_mlkem768(pk_bytes, plaintext)


def test_mlkem_kdf_id():
    """Test ML-KEM KDF ID function."""
    kdf_id = btcr.mlkem_kdf_id()
    assert isinstance(kdf_id, bytes)
    assert kdf_id == b"v1"


def test_encrypt_mlkem768_with_different_plaintexts():
    """Test that encrypt_mlkem768 works with different plaintext sizes."""
    test_cases = [
        b"",  # Empty plaintext
        b"a",  # Single byte
        b"hello",  # Short message
        b"x" * 100,  # Medium message
        b"y" * 1000,  # Large message
    ]

    for plaintext in test_cases:
        ciphertext = btcr.encrypt_mlkem768(VALID_MLKEM768_PK, plaintext)
        assert isinstance(ciphertext, bytes)
        assert len(ciphertext) > 0

        # Verify minimum structure
        assert len(ciphertext) >= 2 + 24  # kem_len + nonce


def test_encrypt_mlkem768_deterministic_commitment():
    """Test that the same plaintext with the same key produces different ciphertexts (nonce is random)."""
    plaintext = b"deterministic test message"

    # Encrypt same plaintext multiple times
    ciphertexts = [
        btcr.encrypt_mlkem768(VALID_MLKEM768_PK, plaintext) for _ in range(5)
    ]

    # All ciphertexts should be different due to random nonce
    assert len(set(ciphertexts)) == 5, (
        "All ciphertexts should be unique due to random nonce"
    )

    # But they should all have the same structure
    for ct in ciphertexts:
        assert len(ct) >= 2 + 24
        kem_len = int.from_bytes(ct[0:2], byteorder="little")
        assert kem_len > 0
