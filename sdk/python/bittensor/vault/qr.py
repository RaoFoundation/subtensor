"""Render UOS frames as QR code images for the vault page.

SVG via the pure-Python ``qrcode`` package — no PIL, no JS encoder to vendor.
Frames are binary (byte-mode QR), exactly as Polkadot Vault's scanner expects.
"""

from __future__ import annotations

import base64

import qrcode
import qrcode.image.svg


def svg_data_uri(frame: bytes) -> str:
    """One QR frame as a ``data:image/svg+xml`` URI ready for an ``<img>``.

    Byte-mode encoding (``optimize=0`` disables mode-splitting heuristics that
    only make sense for text) with the standard 4-module quiet zone.
    """
    qr = qrcode.QRCode(
        error_correction=qrcode.constants.ERROR_CORRECT_M,
        image_factory=qrcode.image.svg.SvgPathImage,
        border=4,
    )
    qr.add_data(frame, optimize=0)
    svg = qr.make_image().to_string()
    return "data:image/svg+xml;base64," + base64.b64encode(svg).decode("ascii")
