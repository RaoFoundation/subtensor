"""Post-quantum account profile advertisement validation."""


def supports_fixed_profile(profiles, profile: int = 5) -> bool:
    """Accept metadata byte arrays as hex, bytes, or decoded integer sequences.

    The native metadata adapter renders [u8; N] constants as hex. Malformed or
    absent advertisements fail closed before receiving setup or private-key use.
    """
    try:
        if isinstance(profiles, str):
            if not profiles.startswith("0x") or len(profiles) > 514:
                return False
            profiles = bytes.fromhex(profiles[2:])
        elif isinstance(profiles, (bytes, bytearray, list, tuple)) and len(profiles) <= 256:
            profiles = bytes(profiles)
        else:
            return False
    except (ValueError, TypeError):
        return False
    return profile in profiles
