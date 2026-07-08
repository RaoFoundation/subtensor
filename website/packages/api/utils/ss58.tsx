import {decodeAddress, encodeAddress} from '@polkadot/keyring';

const prefix = 42;

function decode(address: string) {
  let result;
  try {
    result = decodeAddress(address);
  } catch (e) {
    return null;
  }

  return bytesToHex(result);
}

function encode(address: string) {
  let result;
  try {
    result = encodeAddress(address, prefix);
  } catch (e) {
    return null;
  }

  return result;
}

export const bytesToHex = (bytes: Uint8Array) => {
  return (
    '0x' +
    Array.from(bytes, (byte) => {
      return ('0' + (byte & 0xff).toString(16)).slice(-2);
    }).join('')
  );
};

export const ss58 = {decode, encode};
