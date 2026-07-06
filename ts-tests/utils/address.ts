import { hexToU8a } from "@polkadot/util";
import { blake2AsU8a, decodeAddress, encodeAddress } from "@polkadot/util-crypto";
import { Binary } from "polkadot-api";
import type { Address } from "viem";

const SS58_PREFIX = 42;

export function toViemAddress(address: string): Address {
    const addressNoPrefix = address.replace("0x", "");
    return `0x${addressNoPrefix}`;
}

export function convertH160ToPublicKey(ethAddress: string) {
    const prefix = "evm:";
    const prefixBytes = new TextEncoder().encode(prefix);
    const addressBytes = hexToU8a(ethAddress.startsWith("0x") ? ethAddress : `0x${ethAddress}`);
    const combined = new Uint8Array(prefixBytes.length + addressBytes.length);
    combined.set(prefixBytes);
    combined.set(addressBytes, prefixBytes.length);
    return blake2AsU8a(combined);
}

export function convertH160ToSS58(ethAddress: string): string {
    return encodeAddress(convertH160ToPublicKey(ethAddress), SS58_PREFIX);
}

export function convertPublicKeyToSs58(publicKey: Uint8Array): string {
    return encodeAddress(publicKey, SS58_PREFIX);
}

export function ss58ToEthAddress(ss58Address: string): string {
    const publicKey = decodeAddress(ss58Address);
    const ethereumAddressBytes = publicKey.slice(0, 20);
    return `0x${Buffer.from(ethereumAddressBytes).toString("hex")}`;
}

export function ss58ToH160(ss58Address: string): Binary {
    const publicKey = decodeAddress(ss58Address);
    return new Binary(publicKey.slice(0, 20));
}

export function ethAddressToH160(ethAddress: string): Binary {
    return new Binary(hexToU8a(ethAddress.startsWith("0x") ? ethAddress : `0x${ethAddress}`));
}
