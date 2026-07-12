import native, { type NativeLedgerHandle } from './native'
import { nativeCall } from './errors'
import { toBuffer } from './wire'
import type { ByteLike, LedgerAddress, LedgerVersion } from './types'

export interface LedgerSignerOptions {
  account?: number
  index?: number
  ss58Prefix?: number
  confirmAddress?: boolean
}

export class LedgerSigner {
  readonly requiresMetadataProof = true

  constructor(
    private readonly device: LedgerDevice,
    private readonly options: LedgerSignerOptions = {},
  ) {}

  getAccount(context: { ss58Format?: number } = {}): LedgerAddress {
    return this.device.address(
      this.options.account ?? 0,
      this.options.index ?? 0,
      this.options.ss58Prefix ?? context.ss58Format ?? 42,
      this.options.confirmAddress ?? false,
    )
  }

  signPayload(
    payload: ByteLike,
    context: { proof?: ByteLike; metadataProof?: ByteLike } = {},
  ): Buffer {
    const proof = context.metadataProof ?? context.proof
    if (proof == null) {
      throw new Error('Ledger signing requires an RFC-0078 metadata proof')
    }
    return this.device.sign(
      this.options.account ?? 0,
      this.options.index ?? 0,
      payload,
      proof,
    )
  }
}

export class LedgerDevice {
  private readonly handle: NativeLedgerHandle

  constructor() {
    this.handle = nativeCall(() => native.NativeLedgerDevice.open())
  }

  private static wrap(handle: NativeLedgerHandle): LedgerDevice {
    const device = Object.create(LedgerDevice.prototype) as LedgerDevice
    Object.defineProperty(device, 'handle', { value: handle })
    return device
  }

  static open(): LedgerDevice {
    return LedgerDevice.wrap(nativeCall(() => native.NativeLedgerDevice.open()))
  }

  appVersion(): LedgerVersion {
    return nativeCall(() => this.handle.appVersion())
  }

  app_version(): [number, number, number] {
    const version = this.appVersion()
    return [version.major, version.minor, version.patch]
  }

  address(
    account = 0,
    index = 0,
    ss58Prefix = 42,
    confirm = false,
  ): LedgerAddress {
    return nativeCall(() => this.handle.address(account, index, ss58Prefix, confirm))
  }

  signer(options: LedgerSignerOptions = {}): LedgerSigner {
    return new LedgerSigner(this, options)
  }

  sign(payload: ByteLike, proof: ByteLike, account?: number, index?: number): Buffer
  sign(account: number, index: number, payload: ByteLike, proof: ByteLike): Buffer
  sign(
    first: number | ByteLike,
    second: number | ByteLike,
    third?: number | ByteLike,
    fourth?: number | ByteLike,
  ): Buffer {
    const account = typeof first === 'number' ? first : typeof third === 'number' ? third : 0
    const index = typeof first === 'number' && typeof second === 'number'
      ? second
      : typeof fourth === 'number'
        ? fourth
        : 0
    const payload = typeof first === 'number' ? third : first
    const proof = typeof first === 'number' ? fourth : second
    if (payload == null || proof == null || typeof payload === 'number' || typeof proof === 'number') {
      throw new TypeError('payload and proof are required')
    }
    return nativeCall(() =>
      this.handle.sign(
        account,
        index,
        toBuffer(payload, 'payload'),
        toBuffer(proof, 'proof'),
      ),
    )
  }
}
