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

from .codec import LegacyCodec, RuntimeCodec, ss58_decode
from .errors import SubstrateRequestException
from .rpc import RpcSession

# Bittensor types needed to decode legacy (pre-V15) runtime-call results.
_BITTENSOR_LEGACY_TYPES = {
    "types": {
        "Balance": "u64",
        "StakeInfo": {
            "type": "struct",
            "type_mapping": [
                ["hotkey", "AccountId"],
                ["coldkey", "AccountId"],
                ["stake", "Compact<u64>"],
            ],
        },
    }
}

_legacy_codec: Optional[LegacyCodec] = None


def _legacy() -> LegacyCodec:
    global _legacy_codec
    if _legacy_codec is None:
        _legacy_codec = LegacyCodec(extra_types=_BITTENSOR_LEGACY_TYPES)
    return _legacy_codec


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


def _decode_stake_info_vec(raw: bytes, codec: RuntimeCodec) -> list[dict]:
    """Old ``get_stake_info_for_coldkey`` results, upgraded to the new shape."""
    stake_infos = _legacy().decode("Vec<StakeInfo>", raw)
    return [
        {
            "netuid": 0,
            "hotkey": info["hotkey"],
            "coldkey": info["coldkey"],
            "stake": info["stake"],
            "locked": 0,
            "emission": 0,
            "drain": 0,
            "is_registered": False,
        }
        for info in stake_infos
    ]


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


def _is_legacy_method(codec: RuntimeCodec, api: str, method: str) -> bool:
    """True when the method must go through the legacy path."""
    definition = codec.runtime_api_map.get(api, {}).get(method)
    if definition is None:
        return True  # V14 runtime (or method unknown to V15 metadata)
    return codec.type_name_of(definition["output"]) == "Vec<u8>"


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
    for index, param in enumerate(definition["inputs"]):
        if isinstance(params, list):
            value = params[index]
        else:
            if param["name"] not in params:
                raise ValueError(f"Runtime Call param '{param['name']}' is missing")
            value = params[param["name"]]
        data += codec.encode(f"scale_info::{param['ty']}", value)
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
    return codec.decode(f"scale_info::{definition['output']}", raw)


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
