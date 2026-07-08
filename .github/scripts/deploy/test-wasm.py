import json
import hashlib

with open("subtensor-digest.json", "r") as f:
    digest = json.load(f)

with open("subtensor.wasm", "rb") as f:
    wasm = f.read()

with open("proxy_proxy_blob.hex", "r") as f:
    proxy_proxy_call_data = f.read()

assert wasm.hex() in proxy_proxy_call_data, "WASM not found in proxy_proxy_call_data"

wasm_sha256_sum = hashlib.sha256(wasm).hexdigest()

assert wasm_sha256_sum == digest["sha256"][2:], f"SHA256 mismatch\nExpected {digest['sha256'][2:]}, got {wasm_sha256_sum}"

print("WASM is correct")
