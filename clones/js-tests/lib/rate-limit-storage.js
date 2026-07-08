import { u8aConcat, u8aToHex } from "@polkadot/util";
import { xxhashAsU8a } from "@polkadot/util-crypto";

const SUBTENSOR_MODULE = "SubtensorModule";
const LAST_RATE_LIMITED_BLOCK = "LastRateLimitedBlock";
const NETWORK_LAST_REGISTERED_VARIANT = "0x02";
const MAX_PREFIX_KEYS = 10_000;

export function lastRateLimitedBlockPrefix() {
  return u8aToHex(
    u8aConcat(
      xxhashAsU8a(SUBTENSOR_MODULE, 128),
      xxhashAsU8a(LAST_RATE_LIMITED_BLOCK, 128)
    )
  );
}

export function networkLastRegisteredRateLimitKey() {
  return `${lastRateLimitedBlockPrefix()}${NETWORK_LAST_REGISTERED_VARIANT.slice(2)}`;
}

export async function clearLastRateLimitedBlocks(api, signer, submitAndWait, label = "clear LastRateLimitedBlock") {
  if (!api.tx.sudo?.sudo) {
    throw new Error("Sudo.sudo is unavailable; cannot clear inherited local-clone rate-limit storage");
  }

  const prefix = lastRateLimitedBlockPrefix();
  if (api.tx.system?.killPrefix) {
    await submitAndWait(signer, api.tx.sudo.sudo(api.tx.system.killPrefix(prefix, MAX_PREFIX_KEYS)), label);
    return { mode: "killPrefix", prefix };
  }

  if (api.tx.system?.clearPrefix) {
    await submitAndWait(signer, api.tx.sudo.sudo(api.tx.system.clearPrefix(prefix, MAX_PREFIX_KEYS)), label);
    return { mode: "clearPrefix", prefix };
  }

  if (!api.tx.system?.killStorage) {
    throw new Error("System.killPrefix/clearPrefix/killStorage are unavailable; cannot clear rate-limit storage");
  }

  const key = networkLastRegisteredRateLimitKey();
  await submitAndWait(signer, api.tx.sudo.sudo(api.tx.system.killStorage([key])), `${label} NetworkLastRegistered`);
  return { mode: "killStorage", key };
}
