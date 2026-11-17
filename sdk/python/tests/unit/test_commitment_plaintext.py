import pytest

from bittensor.metagraph import _revealed_entry


@pytest.mark.parametrize(
    "text",
    [
        "affine1|model|https://example.test",
        "(0123456789",
        "hello",
        "🌐模型",
        "0x6869",
        "0xhello",
        "",
    ],
)
def test_raw_revealed_commitments_preserve_every_plaintext_byte(text):
    raw = text.encode()
    for data in (text, raw, bytearray(raw), list(raw)):
        assert _revealed_entry((data, 95)) == (95, text)


def test_legacy_scale_commitments_require_explicit_decoding():
    assert _revealed_entry((b"\x08hi", 95)) == (95, "\x08hi")
    assert _revealed_entry((b"\x08hi", 95), encoding="scale") == (95, "hi")
    for text in ("\x08hi", "0x086869"):
        assert _revealed_entry((text, 95), encoding="scale") == (95, "hi")
    for data in (b"\x08h", b"\x08hi\x00", b"\x09\x00hi", b""):
        with pytest.raises(ValueError, match=r"compact|length|SCALE"):
            _revealed_entry((data, 95), encoding="scale")
    assert _revealed_entry((b"\xff\xfe", 95)) == (95, "0xfffe")
    assert _revealed_entry(("0xfffe", 95)) == (95, "0xfffe")
    assert _revealed_entry((b"\x08\xff\xfe", 95), encoding="scale") == (95, "0xfffe")
