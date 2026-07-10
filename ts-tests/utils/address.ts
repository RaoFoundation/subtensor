import { blake2_256, bytesToHex, hexToBytes, publicKeyFromSs58, ss58FromPublic } from "@bittensor/sdk";
import { Binary } from "polkadot-api";
import type { Address } from "viem";

const SS58_PREFIX = 42;

export function toViemAddress(address: string): Address {
    const addressNoPrefix = address.replace("0x", "");
    return `0x${addressNoPrefix}`;
}

export function convertH160ToPublicKey(ethAddress: string): Uint8Array {
    const addressBytes = hexToBytes(ethAddress, "ethAddress");
    return blake2_256(Buffer.concat([Buffer.from("evm:", "utf8"), addressBytes]));
}

export function convertH160ToSS58(ethAddress: string): string {
    return ss58FromPublic(convertH160ToPublicKey(ethAddress), SS58_PREFIX);
}

export function convertPublicKeyToSs58(publicKey: Uint8Array): string {
    return ss58FromPublic(publicKey, SS58_PREFIX);
}

export function ss58ToEthAddress(ss58Address: string): string {
    return bytesToHex(publicKeyFromSs58(ss58Address).subarray(0, 20));
}

export function ss58ToH160(ss58Address: string): Binary {
    return new Binary(publicKeyFromSs58(ss58Address).subarray(0, 20));
}

export function ethAddressToH160(ethAddress: string): Binary {
    return new Binary(hexToBytes(ethAddress, "ethAddress"));
}
