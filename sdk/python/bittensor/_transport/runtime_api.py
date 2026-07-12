"""Runtime API calls (``state_call``): modern V15 path + legacy registry.

Modern runtimes describe their APIs in V15 metadata, so parameters and results
encode/decode straight from the portable registry. Two situations need the
legacy registry instead:

- the runtime predates V15 metadata entirely (deep archive reads), or
- the method's declared output is ``Vec<u8>`` — Bittensor's old runtime APIs
  returned pre-encoded bytes whose real type only the client knew.

The legacy definitions below are Bittensor-specific and frozen: they only ever
serve historical blocks, so they will never grow.
"""

from __future__ import annotations

from typing import Any, Optional

from .codec import RuntimeCodec, ss58_decode, ss58_encode
from .errors import SubstrateRequestException
from .rpc import RpcSession


def _account_bytes(address: str) -> list[int]:
    return list(bytes.fromhex(ss58_decode(address)))


def _encode_account_vec_u8(params: Any, codec: RuntimeCodec) -> bytes:
    """One SS58 address, passed to the runtime as ``Vec<u8>`` of its raw key."""
    if isinstance(params, str):
        address = params
    elif isinstance(params, list):
        address = params[0]
    else:
        address = params["coldkey_account"]
    return codec.encode("Vec<u8>", _account_bytes(address))


def _encode_accounts_vec_vec_u8(params: Any, codec: RuntimeCodec) -> bytes:
    """Multiple SS58 addresses as ``Vec<Vec<u8>>``."""
    addresses = params[0] if isinstance(params[0], list) else params[0]["coldkey_accounts"]
    return codec.encode("Vec<Vec<u8>>", [_account_bytes(a) for a in addresses])


def _read_compact(data: bytes, offset: int) -> tuple[int, int]:
    """(value, next offset) of one SCALE compact integer."""
    mode = data[offset] & 0x03
    if mode == 0:
        return data[offset] >> 2, offset + 1
    if mode == 1:
        return int.from_bytes(data[offset : offset + 2], "little") >> 2, offset + 2
    if mode == 2:
        return int.from_bytes(data[offset : offset + 4], "little") >> 2, offset + 4
    byte_count = (data[offset] >> 2) + 4
    start = offset + 1
    return int.from_bytes(data[start : start + byte_count], "little"), start + byte_count


def _decode_stake_info_vec(raw: bytes, codec: RuntimeCodec) -> list[dict]:
    """Old ``get_stake_info_for_coldkey`` results, upgraded to the new shape.

    The legacy layout is hand-decoded (``Vec<StakeInfo>`` with StakeInfo =
    ``{hotkey: AccountId, coldkey: AccountId, stake: Compact<u64>}``): it only
    ever serves historical blocks, so it is frozen and will never grow.
    """
    count, offset = _read_compact(raw, 0)
    out = []
    for _ in range(count):
        hotkey = ss58_encode(raw[offset : offset + 32], codec.ss58_format)
        coldkey = ss58_encode(raw[offset + 32 : offset + 64], codec.ss58_format)
        stake, offset = _read_compact(raw, offset + 64)
        out.append(
            {
                "netuid": 0,
                "hotkey": hotkey,
                "coldkey": coldkey,
                "stake": stake,
                "locked": 0,
                "emission": 0,
                "drain": 0,
                "is_registered": False,
            }
        )
    return out


def _registry_decoder(type_name: str):
    def decode(raw: bytes, codec: RuntimeCodec) -> Any:
        return codec.decode_by_type_name(type_name, raw)

    return decode


# {api: {method: {"params": [{"name", "type"}...], "encoder"?: fn, "decoder": fn}}}
LEGACY_RUNTIME_APIS: dict[str, dict[str, dict]] = {
    "DelegateInfoRuntimeApi": {
        "get_delegated": {
            "params": [{"name": "coldkey", "type": "Vec<u8>"}],
            "encoder": _encode_account_vec_u8,
            "decoder": _registry_decoder("Vec<DelegateInfo>"),
        },
        "get_delegates": {
            "params": [],
            "decoder": _registry_decoder("Vec<DelegateInfo>"),
        },
    },
    "NeuronInfoRuntimeApi": {
        "get_neuron_lite": {
            "params": [{"name": "netuid", "type": "u16"}, {"name": "uid", "type": "u16"}],
            "decoder": _registry_decoder("NeuronInfoLite"),
        },
        "get_neurons_lite": {
            "params": [{"name": "netuid", "type": "u16"}],
            "decoder": _registry_decoder("Vec<NeuronInfoLite>"),
        },
        "get_neuron": {
            "params": [{"name": "netuid", "type": "u16"}, {"name": "uid", "type": "u16"}],
            "decoder": _registry_decoder("NeuronInfo"),
        },
        "get_neurons": {
            "params": [{"name": "netuid", "type": "u16"}],
            "decoder": _registry_decoder("Vec<NeuronInfo>"),
        },
    },
    "StakeInfoRuntimeApi": {
        "get_stake_info_for_coldkey": {
            "params": [{"name": "coldkey_account_vec", "type": "Vec<u8>"}],
            "encoder": _encode_account_vec_u8,
            "decoder": _decode_stake_info_vec,
        },
        "get_stake_info_for_coldkeys": {
            "params": [{"name": "coldkey_account_vecs", "type": "Vec<Vec<u8>>"}],
            "encoder": _encode_accounts_vec_vec_u8,
            "decoder": _registry_decoder("Vec<(Vec<u8>, Vec<StakeInfo>)>"),
        },
    },
    "SubnetInfoRuntimeApi": {
        "get_subnet_hyperparams": {
            "params": [{"name": "netuid", "type": "u16"}],
            "decoder": _registry_decoder("Option<SubnetHyperparameters>"),
        },
        "get_subnet_info": {
            "params": [{"name": "netuid", "type": "u16"}],
            "decoder": _registry_decoder("Option<SubnetInfo>"),
        },
        "get_subnet_info_v2": {
            "params": [{"name": "netuid", "type": "u16"}],
            "decoder": _registry_decoder("Option<SubnetInfoV2>"),
        },
        "get_subnets_info": {
            "params": [],
            "decoder": _registry_decoder("Vec<Option<SubnetInfo>>"),
        },
        "get_subnets_info_v2": {
            "params": [],
            "decoder": _registry_decoder("Vec<Option<SubnetInfo>>"),
        },
    },
}


def _output_type_name(codec: RuntimeCodec, definition: dict) -> Optional[str]:
    type_id = int(definition["output"].removeprefix("scale_info::"))
    return codec.type_name_of(type_id)


def _is_legacy_method(codec: RuntimeCodec, api: str, method: str) -> bool:
    """True when the method must go through the legacy path."""
    definition = codec.runtime_api_map.get(api, {}).get(method)
    if definition is None:
        return True  # V14 runtime (or method unknown to V15 metadata)
    return _output_type_name(codec, definition) == "Vec<u8>"


def encode_runtime_api_params(
    codec: RuntimeCodec, api: str, method: str, params: list | dict
) -> str:
    """Hex-encoded parameter bytes for a ``state_call``."""
    if _is_legacy_method(codec, api, method):
        definition = _legacy_definition(api, method)
        encoder = definition.get("encoder")
        if encoder is not None:
            return encoder(params, codec).hex()
        data = b""
        for index, param in enumerate(definition["params"]):
            value = params[index] if isinstance(params, list) else params[param["name"]]
            data += codec.encode(param["type"], value)
        return data.hex()

    definition = codec.runtime_api_map[api][method]
    if isinstance(params, list) and len(params) != len(definition["inputs"]):
        raise ValueError(
            f"Number of parameters provided ({len(params)}) does not match "
            f"definition {len(definition['inputs'])} for '{api}.{method}'"
        )
    data = b""
    for index, (name, type_string) in enumerate(definition["inputs"]):
        if isinstance(params, list):
            value = params[index]
        else:
            if name not in params:
                raise ValueError(f"Runtime Call param '{name}' is missing")
            value = params[name]
        data += codec.encode(type_string, value)
    return data.hex()


def decode_runtime_api_result(codec: RuntimeCodec, api: str, method: str, raw: bytes) -> Any:
    if _is_legacy_method(codec, api, method):
        definition = _legacy_definition(api, method)
        # Legacy results arrive double-encoded: a Vec<u8> whose payload is the
        # real SCALE value.
        inner = codec.decode("Vec<u8>", raw)
        inner_bytes = (
            bytes.fromhex(inner.removeprefix("0x")) if isinstance(inner, str) else bytes(inner)
        )
        return definition["decoder"](inner_bytes, codec)
    definition = codec.runtime_api_map[api][method]
    return codec.decode(definition["output"], raw)


def _legacy_definition(api: str, method: str) -> dict:
    try:
        return LEGACY_RUNTIME_APIS[api][method]
    except KeyError:
        raise ValueError(f"Runtime API Call '{api}.{method}' not found in registry") from None


async def call_runtime_api(
    session: RpcSession,
    codec: RuntimeCodec,
    api: str,
    method: str,
    params: Optional[list | dict],
    block_hash: Optional[str],
) -> Any:
    """Encode, execute, and decode one runtime API call."""
    params_hex = encode_runtime_api_params(codec, api, method, params or [])
    result = await session.request("state_call", [f"{api}_{method}", params_hex, block_hash])
    if result is None:
        raise SubstrateRequestException(f"no result from runtime call {api}.{method}")
    return decode_runtime_api_result(codec, api, method, bytes.fromhex(result.removeprefix("0x")))
