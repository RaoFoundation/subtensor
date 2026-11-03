import native, {
  type NativePowGpuDevice,
  type NativePowGpuMinerHandle,
  type NativePowSolution,
} from './native'
import { nativeAsync, nativeCall } from './errors'
import { toBuffer } from './wire'
import type { ByteLike, IntegerLike } from './types'
import { publicKeyFromSs58 } from './keys'
import { mineRegistration, type MineRegistrationOptions, type RegistrationReader } from './pow-registration'
import type { Intent } from './intents'

export type PowSolution = NativePowSolution
export type PowGpuDevice = NativePowGpuDevice

export const POW_PREFIX_LENGTH = native.powPrefixLength()
export const POW_MAX_BATCH = native.powMaxBatch()

function u64(value: IntegerLike, name: string): bigint {
  if (typeof value === 'number' && !Number.isSafeInteger(value)) {
    throw new RangeError(`${name} must be a safe integer, bigint, or decimal string`)
  }
  if (typeof value === 'string' && !/^(0|[1-9][0-9]*)$/.test(value)) {
    throw new RangeError(`${name} must be an unsigned decimal integer`)
  }
  const result = BigInt(value)
  if (result < 0n || result > 0xffffffffffffffffn) throw new RangeError(`${name} must fit u64`)
  return result
}

function attemptsValue(attempts: number): number {
  if (!Number.isSafeInteger(attempts) || attempts < 1 || attempts > POW_MAX_BATCH) {
    throw new RangeError(`attempts must be in 1..${POW_MAX_BATCH}`)
  }
  return attempts
}

/** Compute a registration seal from a public challenge prefix and nonce. */
export function powSeal(prefix: ByteLike, nonce: IntegerLike): Buffer {
  return nativeCall(() => native.powSeal(toBuffer(prefix, 'prefix'), u64(nonce, 'nonce')))
}

/** Search one bounded batch off the JS event loop. Refresh chain challenges between batches. */
export function powMineCpu(
  prefix: ByteLike,
  difficulty: IntegerLike,
  start: IntegerLike = 0n,
  attempts = POW_MAX_BATCH,
): Promise<PowSolution | null> {
  return nativeAsync(() => native.powMineCpu(
    toBuffer(prefix, 'prefix'), u64(difficulty, 'difficulty'), u64(start, 'start'), attemptsValue(attempts),
  ))
}

/** Enumerate optional OpenCL GPU devices. */
export function powGpuDevices(): Promise<PowGpuDevice[]> {
  return nativeAsync(() => native.powGpuDevices())
}

/** Persistent GPU contexts; only public challenge bytes enter device memory. */
export class PowGpuMiner {
  private constructor(private readonly native: NativePowGpuMinerHandle) {}

  static async create(deviceIds?: number[]): Promise<PowGpuMiner> {
    if (deviceIds != null && (deviceIds.length === 0 || deviceIds.length > 32 ||
      new Set(deviceIds).size !== deviceIds.length ||
      deviceIds.some((id) => !Number.isSafeInteger(id) || id < 0 || id > 0xffffffff))) {
      throw new RangeError('select 1..32 unique unsigned GPU device ids')
    }
    return new PowGpuMiner(await nativeAsync(() => native.NativePowGpuMiner.create(deviceIds)))
  }

  devices(): PowGpuDevice[] {
    return nativeCall(() => this.native.devices())
  }

  mine(prefix: ByteLike, difficulty: IntegerLike, start: IntegerLike = 0n, attemptsPerDevice = POW_MAX_BATCH): Promise<PowSolution | null> {
    return nativeAsync(() => this.native.mine(
      toBuffer(prefix, 'prefix'), u64(difficulty, 'difficulty'), u64(start, 'start'), attemptsValue(attemptsPerDevice),
    ))
  }
}

/** CPU/GPU registration mining with live challenge and difficulty refresh. */
export async function minePowRegistration(reader: RegistrationReader, netuid: number, hotkey: string, coldkey: string, options: MineRegistrationOptions = {}): Promise<Intent> {
  const backend = options.backend ?? 'auto'
  let gpu: PowGpuMiner | undefined
  if (backend !== 'cpu') {
    const devices = await powGpuDevices()
    if (devices.length) gpu = await PowGpuMiner.create(options.deviceIds)
    else if (backend === 'gpu' || options.deviceIds != null) throw new Error('No usable OpenCL GPU found')
  }
  return mineRegistration(reader, netuid, hotkey, coldkey, {
    decodeAddress: publicKeyFromSs58,
    seal: powSeal,
    mine: gpu ? (prefix, difficulty, start, attempts) => gpu!.mine(prefix, difficulty, start, attempts) : powMineCpu,
    parallelism: gpu?.devices().length,
  }, options)
}
