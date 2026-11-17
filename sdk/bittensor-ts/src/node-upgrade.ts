import { readFile } from 'node:fs/promises'
import { fetchUpgradeBundle, type UpgradeBundle, type UpgradeFetchOptions } from './upgrade'
/** Read local, independently reproduced artifacts; remote bundle support is shared with the browser. */
export async function fetchUpgradeBundleFromFiles(source: { releaseUrl?: string; hexUrl?: string; hexFile?: string; wasmPath?: string }, options: UpgradeFetchOptions = {}): Promise<UpgradeBundle> {
  const hex = source.hexFile == null ? undefined : await readFile(source.hexFile)
  const wasm = source.wasmPath == null ? undefined : await readFile(source.wasmPath)
  return fetchUpgradeBundle({ releaseUrl: source.releaseUrl, hexUrl: source.hexUrl, hex, wasm }, options)
}

import type { Client } from './client'
import { UpgradeClient, type UpgradeAdapter } from './upgrade'
import { encodeIntentCall } from './intents'
import { blake2_256 } from './crypto'
import { publicKeyFromSs58, ss58FromPublic } from './keys'
import { multisigAccountId } from './runtime'
import type { ScaleValue } from './types'
/** Uses one finalized snapshot for discovery, metadata reconstruction and approval planning. */
export function createNodeUpgradeClient(client: Client): UpgradeClient {
  const adapter = (hash?: string): UpgradeAdapter => ({
    query: (item, params = []) => client.query(item, params as ScaleValue[], hash),
    queryMap: (item, params = []) => client.queryMap(item, params as ScaleValue[], hash),
    compose: call => call instanceof Uint8Array ? Promise.resolve(call.slice()) : encodeIntentCall(call, async (pallet, fn, params) => client.composeCall(pallet, fn, params as ScaleValue, hash)),
    crypto: { blake2_256, publicKeyFromSs58, ss58FromPublic },
    multisig: (signatories, threshold) => ({ address: ss58FromPublic(multisigAccountId(signatories.map(publicKeyFromSs58), threshold).accountId) }),
    at: async () => hash == null ? adapter(await client.finalizedHead()) : adapter(hash),
    estimateWeight: async (call, address) => {
      const runtime = await client.runtimeAt(hash)
      const extrinsic = runtime.encodeSignedExtrinsic(call, publicKeyFromSs58(address), new Uint8Array(64), 1, { era: '00', nonce: 0, tip: 0n })
      const info = await client.rpc('payment_queryInfo', ['0x' + Buffer.from(extrinsic.bytes).toString('hex'), hash]) as any
      if (info?.weight == null) throw new Error('upgrade finalization weight estimate unavailable')
      return typeof info.weight === 'object' ? { ref_time: BigInt(info.weight.ref_time ?? info.weight.refTime), proof_size: BigInt(info.weight.proof_size ?? info.weight.proofSize ?? 0) } : { ref_time: BigInt(info.weight), proof_size: 0n }
    },
  })
  return new UpgradeClient(adapter())
}
