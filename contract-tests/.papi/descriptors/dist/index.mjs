import {
  __export
} from "./chunk-7P6ASYW6.mjs";

// .papi/descriptors/src/common.ts
var table = new Uint8Array(128);
for (let i = 0; i < 64; i++) table[i < 26 ? i + 65 : i < 52 ? i + 71 : i < 62 ? i - 4 : i * 4 - 205] = i;
var toBinary = (base64) => {
  const n = base64.length, bytes = new Uint8Array((n - Number(base64[n - 1] === "=") - Number(base64[n - 2] === "=")) * 3 / 4 | 0);
  for (let i2 = 0, j = 0; i2 < n; ) {
    const c0 = table[base64.charCodeAt(i2++)], c1 = table[base64.charCodeAt(i2++)];
    const c2 = table[base64.charCodeAt(i2++)], c3 = table[base64.charCodeAt(i2++)];
    bytes[j++] = c0 << 2 | c1 >> 4;
    bytes[j++] = c1 << 4 | c2 >> 2;
    bytes[j++] = c2 << 6 | c3;
  }
  return bytes;
};

// .papi/descriptors/src/devnet.ts
var descriptorValues = import("./descriptors-UMN7HDMZ.mjs").then((module) => module["Devnet"]);
var metadataTypes = import("./metadataTypes-4SGFT3Q5.mjs").then(
  (module) => toBinary("default" in module ? module.default : module)
);
var asset = {};
var extensions = {};
var getMetadata = () => import("./devnet_metadata-NGDS3DPZ.mjs").then(
  (module) => toBinary("default" in module ? module.default : module)
);
var genesis = "0x56c04ae0094e08e671ff781eb7122ffca041a55d208befc4603bd441b44e8e3a";
var _allDescriptors = { descriptors: descriptorValues, metadataTypes, asset, extensions, getMetadata, genesis };
var devnet_default = _allDescriptors;

// .papi/descriptors/src/common-types.ts
import { _Enum } from "polkadot-api";
var DigestItem = _Enum;
var Phase = _Enum;
var DispatchClass = _Enum;
var TokenError = _Enum;
var ArithmeticError = _Enum;
var TransactionalError = _Enum;
var GrandpaEvent = _Enum;
var BalanceStatus = _Enum;
var TransactionPaymentEvent = _Enum;
var PreimageEvent = _Enum;
var GrandpaStoredState = _Enum;
var BalancesTypesReasons = _Enum;
var PreimagePalletHoldReason = _Enum;
var TransactionPaymentReleases = _Enum;
var PreimageOldRequestStatus = _Enum;
var PreimageRequestStatus = _Enum;
var PreimagesBounded = _Enum;
var GrandpaEquivocation = _Enum;
var MultiAddress = _Enum;
var BalancesAdjustmentDirection = _Enum;
var MultiSigner = _Enum;
var MultiSignature = _Enum;
var TransactionValidityUnknownTransaction = _Enum;
var TransactionValidityTransactionSource = _Enum;
var BabeAllowedSlots = _Enum;

// .papi/descriptors/src/contracts/index.ts
var contracts_exports = {};
__export(contracts_exports, {
  bittensor: () => descriptor
});

// .papi/descriptors/src/contracts/bittensor.ts
var descriptor = { metadata: { "source": { "hash": "0xd15a29edfb7134dface2e0b6c704c173d47d3b48f1afc87193922ff8114e457d", "language": "ink! 5.1.1", "compiler": "rustc 1.89.0", "build_info": { "build_mode": "Release", "cargo_contract_version": "5.0.3", "rust_toolchain": "stable-aarch64-apple-darwin", "wasm_opt_settings": { "keep_debug_symbols": false, "optimization_passes": "Z" } } }, "contract": { "name": "bittensor", "version": "0.1.0", "authors": ["[your_name] <[your_email]>"] }, "image": null, "spec": { "constructors": [{ "args": [], "default": false, "docs": ["Constructor"], "label": "new", "payable": false, "returnType": { "displayName": ["ink_primitives", "ConstructorResult"], "type": 1 }, "selector": "0x9bae9d5e" }, { "args": [], "default": false, "docs": ["Constructor"], "label": "default", "payable": false, "returnType": { "displayName": ["ink_primitives", "ConstructorResult"], "type": 1 }, "selector": "0xed4b9d1b" }], "docs": [], "environment": { "accountId": { "displayName": ["AccountId"], "type": 11 }, "balance": { "displayName": ["Balance"], "type": 14 }, "blockNumber": { "displayName": ["BlockNumber"], "type": 24 }, "chainExtension": { "displayName": ["ChainExtension"], "type": 25 }, "hash": { "displayName": ["Hash"], "type": 23 }, "maxEventTopics": 4, "staticBufferSize": 16384, "timestamp": { "displayName": ["Timestamp"], "type": 14 } }, "events": [], "lang_error": { "displayName": ["ink", "LangError"], "type": 3 }, "messages": [{ "args": [{ "label": "hotkey", "type": { "displayName": [], "type": 4 } }, { "label": "coldkey", "type": { "displayName": [], "type": 4 } }, { "label": "netuid", "type": { "displayName": ["u16"], "type": 6 } }], "default": false, "docs": [], "label": "get_stake_info_for_hotkey_coldkey_netuid", "mutates": false, "payable": false, "returnType": { "displayName": ["ink", "MessageResult"], "type": 7 }, "selector": "0x5b73b8b9" }, { "args": [{ "label": "hotkey", "type": { "displayName": [], "type": 4 } }, { "label": "netuid", "type": { "displayName": ["u16"], "type": 6 } }, { "label": "amount", "type": { "displayName": ["u64"], "type": 14 } }], "default": false, "docs": [], "label": "add_stake", "mutates": false, "payable": false, "returnType": { "displayName": ["ink", "MessageResult"], "type": 19 }, "selector": "0x3a656e31" }, { "args": [{ "label": "hotkey", "type": { "displayName": [], "type": 4 } }, { "label": "netuid", "type": { "displayName": ["u16"], "type": 6 } }, { "label": "amount", "type": { "displayName": ["u64"], "type": 14 } }], "default": false, "docs": [], "label": "remove_stake", "mutates": false, "payable": false, "returnType": { "displayName": ["ink", "MessageResult"], "type": 19 }, "selector": "0x7758d434" }, { "args": [{ "label": "hotkey", "type": { "displayName": [], "type": 4 } }], "default": false, "docs": [], "label": "unstake_all", "mutates": false, "payable": false, "returnType": { "displayName": ["ink", "MessageResult"], "type": 19 }, "selector": "0x3f525cc7" }, { "args": [{ "label": "hotkey", "type": { "displayName": [], "type": 4 } }], "default": false, "docs": [], "label": "unstake_all_alpha", "mutates": false, "payable": false, "returnType": { "displayName": ["ink", "MessageResult"], "type": 19 }, "selector": "0xab74c422" }, { "args": [{ "label": "origin_hotkey", "type": { "displayName": [], "type": 4 } }, { "label": "destination_hotkey", "type": { "displayName": [], "type": 4 } }, { "label": "origin_netuid", "type": { "displayName": ["u16"], "type": 6 } }, { "label": "destination_netuid", "type": { "displayName": ["u16"], "type": 6 } }, { "label": "amount", "type": { "displayName": ["u64"], "type": 14 } }], "default": false, "docs": [], "label": "move_stake", "mutates": false, "payable": false, "returnType": { "displayName": ["ink", "MessageResult"], "type": 19 }, "selector": "0xa06b0c55" }, { "args": [{ "label": "destination_coldkey", "type": { "displayName": [], "type": 4 } }, { "label": "hotkey", "type": { "displayName": [], "type": 4 } }, { "label": "origin_netuid", "type": { "displayName": ["u16"], "type": 6 } }, { "label": "destination_netuid", "type": { "displayName": ["u16"], "type": 6 } }, { "label": "amount", "type": { "displayName": ["u64"], "type": 14 } }], "default": false, "docs": [], "label": "transfer_stake", "mutates": false, "payable": false, "returnType": { "displayName": ["ink", "MessageResult"], "type": 19 }, "selector": "0x3528ef5e" }, { "args": [{ "label": "hotkey", "type": { "displayName": [], "type": 4 } }, { "label": "origin_netuid", "type": { "displayName": ["u16"], "type": 6 } }, { "label": "destination_netuid", "type": { "displayName": ["u16"], "type": 6 } }, { "label": "amount", "type": { "displayName": ["u64"], "type": 14 } }], "default": false, "docs": [], "label": "swap_stake", "mutates": false, "payable": false, "returnType": { "displayName": ["ink", "MessageResult"], "type": 19 }, "selector": "0x04f7ca30" }, { "args": [{ "label": "hotkey", "type": { "displayName": [], "type": 4 } }, { "label": "netuid", "type": { "displayName": ["u16"], "type": 6 } }, { "label": "amount", "type": { "displayName": ["u64"], "type": 14 } }, { "label": "limit_price", "type": { "displayName": ["u64"], "type": 14 } }, { "label": "allow_partial", "type": { "displayName": ["bool"], "type": 17 } }], "default": false, "docs": [], "label": "add_stake_limit", "mutates": false, "payable": false, "returnType": { "displayName": ["ink", "MessageResult"], "type": 19 }, "selector": "0x30013b98" }, { "args": [{ "label": "hotkey", "type": { "displayName": [], "type": 4 } }, { "label": "netuid", "type": { "displayName": ["u16"], "type": 6 } }, { "label": "amount", "type": { "displayName": ["u64"], "type": 14 } }, { "label": "limit_price", "type": { "displayName": ["u64"], "type": 14 } }, { "label": "allow_partial", "type": { "displayName": ["bool"], "type": 17 } }], "default": false, "docs": [], "label": "remove_stake_limit", "mutates": false, "payable": false, "returnType": { "displayName": ["ink", "MessageResult"], "type": 19 }, "selector": "0xc3ce39c8" }, { "args": [{ "label": "hotkey", "type": { "displayName": [], "type": 4 } }, { "label": "origin_netuid", "type": { "displayName": ["u16"], "type": 6 } }, { "label": "destination_netuid", "type": { "displayName": ["u16"], "type": 6 } }, { "label": "amount", "type": { "displayName": ["u64"], "type": 14 } }, { "label": "limit_price", "type": { "displayName": ["u64"], "type": 14 } }, { "label": "allow_partial", "type": { "displayName": ["bool"], "type": 17 } }], "default": false, "docs": [], "label": "swap_stake_limit", "mutates": false, "payable": false, "returnType": { "displayName": ["ink", "MessageResult"], "type": 19 }, "selector": "0x212ef7ad" }, { "args": [{ "label": "hotkey", "type": { "displayName": [], "type": 4 } }, { "label": "netuid", "type": { "displayName": ["u16"], "type": 6 } }, { "label": "limit_price", "type": { "displayName": ["u64"], "type": 14 } }], "default": false, "docs": [], "label": "remove_stake_full_limit", "mutates": false, "payable": false, "returnType": { "displayName": ["ink", "MessageResult"], "type": 19 }, "selector": "0xa6d6ea64" }, { "args": [{ "label": "netuid", "type": { "displayName": ["u16"], "type": 6 } }, { "label": "hotkey", "type": { "displayName": [], "type": 4 } }], "default": false, "docs": [], "label": "set_coldkey_auto_stake_hotkey", "mutates": false, "payable": false, "returnType": { "displayName": ["ink", "MessageResult"], "type": 19 }, "selector": "0xe24f0d8a" }, { "args": [{ "label": "delegate", "type": { "displayName": [], "type": 4 } }], "default": false, "docs": [], "label": "add_proxy", "mutates": false, "payable": false, "returnType": { "displayName": ["ink", "MessageResult"], "type": 19 }, "selector": "0x528b6757" }, { "args": [{ "label": "delegate", "type": { "displayName": [], "type": 4 } }], "default": false, "docs": [], "label": "remove_proxy", "mutates": false, "payable": false, "returnType": { "displayName": ["ink", "MessageResult"], "type": 19 }, "selector": "0x129d4f75" }, { "args": [{ "label": "netuid", "type": { "displayName": ["u16"], "type": 6 } }], "default": false, "docs": [], "label": "get_alpha_price", "mutates": false, "payable": false, "returnType": { "displayName": ["ink", "MessageResult"], "type": 21 }, "selector": "0x08adc2e2" }] }, "storage": { "root": { "layout": { "struct": { "fields": [], "name": "Bittensor" } }, "root_key": "0x00000000", "ty": 0 } }, "types": [{ "id": 0, "type": { "def": { "composite": {} }, "path": ["bittensor", "bittensor", "Bittensor"] } }, { "id": 1, "type": { "def": { "variant": { "variants": [{ "fields": [{ "type": 2 }], "index": 0, "name": "Ok" }, { "fields": [{ "type": 3 }], "index": 1, "name": "Err" }] } }, "params": [{ "name": "T", "type": 2 }, { "name": "E", "type": 3 }], "path": ["Result"] } }, { "id": 2, "type": { "def": { "tuple": [] } } }, { "id": 3, "type": { "def": { "variant": { "variants": [{ "index": 1, "name": "CouldNotReadInput" }] } }, "path": ["ink_primitives", "LangError"] } }, { "id": 4, "type": { "def": { "array": { "len": 32, "type": 5 } } } }, { "id": 5, "type": { "def": { "primitive": "u8" } } }, { "id": 6, "type": { "def": { "primitive": "u16" } } }, { "id": 7, "type": { "def": { "variant": { "variants": [{ "fields": [{ "type": 8 }], "index": 0, "name": "Ok" }, { "fields": [{ "type": 3 }], "index": 1, "name": "Err" }] } }, "params": [{ "name": "T", "type": 8 }, { "name": "E", "type": 3 }], "path": ["Result"] } }, { "id": 8, "type": { "def": { "variant": { "variants": [{ "fields": [{ "type": 9 }], "index": 0, "name": "Ok" }, { "fields": [{ "type": 18 }], "index": 1, "name": "Err" }] } }, "params": [{ "name": "T", "type": 9 }, { "name": "E", "type": 18 }], "path": ["Result"] } }, { "id": 9, "type": { "def": { "variant": { "variants": [{ "index": 0, "name": "None" }, { "fields": [{ "type": 10 }], "index": 1, "name": "Some" }] } }, "params": [{ "name": "T", "type": 10 }], "path": ["Option"] } }, { "id": 10, "type": { "def": { "composite": { "fields": [{ "name": "hotkey", "type": 11, "typeName": "AccountId" }, { "name": "coldkey", "type": 11, "typeName": "AccountId" }, { "name": "netuid", "type": 12, "typeName": "Compact<NetUid>" }, { "name": "stake", "type": 13, "typeName": "Compact<AlphaBalance>" }, { "name": "locked", "type": 15, "typeName": "Compact<u64>" }, { "name": "emission", "type": 13, "typeName": "Compact<AlphaBalance>" }, { "name": "tao_emission", "type": 16, "typeName": "Compact<TaoBalance>" }, { "name": "drain", "type": 15, "typeName": "Compact<u64>" }, { "name": "is_registered", "type": 17, "typeName": "bool" }] } }, "params": [{ "name": "AccountId", "type": 11 }], "path": ["bittensor", "StakeInfo"] } }, { "id": 11, "type": { "def": { "composite": { "fields": [{ "type": 4, "typeName": "[u8; 32]" }] } }, "path": ["ink_primitives", "types", "AccountId"] } }, { "id": 12, "type": { "def": { "compact": { "type": 6 } } } }, { "id": 13, "type": { "def": { "compact": { "type": 14 } } } }, { "id": 14, "type": { "def": { "primitive": "u64" } } }, { "id": 15, "type": { "def": { "compact": { "type": 14 } } } }, { "id": 16, "type": { "def": { "compact": { "type": 14 } } } }, { "id": 17, "type": { "def": { "primitive": "bool" } } }, { "id": 18, "type": { "def": { "variant": { "variants": [{ "index": 0, "name": "ReadFailed" }, { "index": 1, "name": "WriteFailed" }] } }, "path": ["bittensor", "ReadWriteErrorCode"] } }, { "id": 19, "type": { "def": { "variant": { "variants": [{ "fields": [{ "type": 20 }], "index": 0, "name": "Ok" }, { "fields": [{ "type": 3 }], "index": 1, "name": "Err" }] } }, "params": [{ "name": "T", "type": 20 }, { "name": "E", "type": 3 }], "path": ["Result"] } }, { "id": 20, "type": { "def": { "variant": { "variants": [{ "fields": [{ "type": 2 }], "index": 0, "name": "Ok" }, { "fields": [{ "type": 18 }], "index": 1, "name": "Err" }] } }, "params": [{ "name": "T", "type": 2 }, { "name": "E", "type": 18 }], "path": ["Result"] } }, { "id": 21, "type": { "def": { "variant": { "variants": [{ "fields": [{ "type": 22 }], "index": 0, "name": "Ok" }, { "fields": [{ "type": 3 }], "index": 1, "name": "Err" }] } }, "params": [{ "name": "T", "type": 22 }, { "name": "E", "type": 3 }], "path": ["Result"] } }, { "id": 22, "type": { "def": { "variant": { "variants": [{ "fields": [{ "type": 14 }], "index": 0, "name": "Ok" }, { "fields": [{ "type": 18 }], "index": 1, "name": "Err" }] } }, "params": [{ "name": "T", "type": 14 }, { "name": "E", "type": 18 }], "path": ["Result"] } }, { "id": 23, "type": { "def": { "composite": { "fields": [{ "type": 4, "typeName": "[u8; 32]" }] } }, "path": ["ink_primitives", "types", "Hash"] } }, { "id": 24, "type": { "def": { "primitive": "u32" } } }, { "id": 25, "type": { "def": { "variant": {} }, "path": ["bittensor", "RuntimeReadWrite"] } }], "version": 5 } };

// .papi/descriptors/src/index.ts
var metadatas = { ["0xf3fc94ee2cf40e8a3e48a4d88d4880d606a706312512f6e0317eba65e032a0d8"]: devnet_default };
var getMetadata2 = async (codeHash) => {
  try {
    return await metadatas[codeHash].getMetadata();
  } catch {
  }
  return null;
};
export {
  ArithmeticError,
  BabeAllowedSlots,
  BalanceStatus,
  BalancesAdjustmentDirection,
  BalancesTypesReasons,
  DigestItem,
  DispatchClass,
  GrandpaEquivocation,
  GrandpaEvent,
  GrandpaStoredState,
  MultiAddress,
  MultiSignature,
  MultiSigner,
  Phase,
  PreimageEvent,
  PreimageOldRequestStatus,
  PreimagePalletHoldReason,
  PreimageRequestStatus,
  PreimagesBounded,
  TokenError,
  TransactionPaymentEvent,
  TransactionPaymentReleases,
  TransactionValidityTransactionSource,
  TransactionValidityUnknownTransaction,
  TransactionalError,
  contracts_exports as contracts,
  devnet_default as devnet,
  getMetadata2 as getMetadata
};
