import hashlib
import json

with open("subtensor-digest.json", "r") as f:
    digest = json.load(f)

with open("subtensor.wasm", "rb") as f:
    wasm = f.read()

with open("proxy_proxy_blob.hex", "r") as f:
    proxy_proxy_call_data = f.read()


def compact(n: int) -> bytes:
    """SCALE compact encoding of a non-negative integer."""
    if n < 1 << 6:
        return bytes([n << 2])
    if n < 1 << 14:
        return ((n << 2) | 0b01).to_bytes(2, "little")
    if n < 1 << 30:
        return ((n << 2) | 0b10).to_bytes(4, "little")
    data = n.to_bytes((n.bit_length() + 7) // 8, "little")
    return bytes([((len(data) - 4) << 2) | 0b11]) + data


# The blob is the bare SCALE encoding of
#   proxy.proxy(real=sudo_key, force_proxy_type=None,
#               sudo.sudoUncheckedWeight(system.setCode(wasm), WEIGHT))
# as built by propose-upgrade-multisig.js. Rebuild that encoding here and
# require the blob to match it byte for byte, so the WASM must be the `code`
# argument of setCode and the call can contain nothing else (a batch or an
# extra call appended around honest WASM bytes fails this check). The only
# part not pinned is the 32-byte `real` account, which is the on-chain sudo
# key and not knowable offline; everything around it is exact.
#
# Pallet/call indices come from the runtime (System=0, Sudo=12, Proxy=16) and
# the weight literal from propose-upgrade-multisig.js; if either changes, this
# check fails loudly and must be updated in lockstep.
PROXY_PROXY = bytes([16, 0])
MULTIADDRESS_ID = bytes([0])
REAL_LEN = 32
OPTION_NONE = bytes([0])
SUDO_SUDO_UNCHECKED_WEIGHT = bytes([12, 1])
SYSTEM_SET_CODE = bytes([0, 2])
WEIGHT = compact(50_000_000_000) + compact(0)  # {refTime, proofSize}

blob = bytes.fromhex(proxy_proxy_call_data.strip().removeprefix("0x"))

head = PROXY_PROXY + MULTIADDRESS_ID
assert blob[: len(head)] == head, "call data is not proxy.proxy(MultiAddress::Id(...), ...)"

tail = (
    OPTION_NONE
    + SUDO_SUDO_UNCHECKED_WEIGHT
    + SYSTEM_SET_CODE
    + compact(len(wasm))
    + wasm
    + WEIGHT
)
assert blob[len(head) + REAL_LEN :] == tail, (
    "call data after the proxied account is not exactly "
    "sudoUncheckedWeight(setCode(<this WASM>), WEIGHT)"
)

wasm_sha256_sum = hashlib.sha256(wasm).hexdigest()
digest_sha256 = digest["sha256"].removeprefix("0x").lower()

assert wasm_sha256_sum == digest_sha256, (
    f"SHA256 mismatch\nExpected {digest_sha256}, got {wasm_sha256_sum}"
)

print("WASM is correct")
