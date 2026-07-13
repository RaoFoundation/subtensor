import native, { type NativeLedgerHandle } from './native'
import { nativeAsync } from './errors'
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

  getAccount(context: { ss58Format?: number } = {}): Promise<LedgerAddress> {
    return this.device.address(
      this.options.account ?? 0,
      this.options.index ?? 0,
      this.options.ss58Prefix ?? context.ss58Format ?? 42,
      this.options.confirmAddress ?? false,
    )
  }

  signBytes(
    payload: ByteLike,
    context: { proof?: ByteLike; metadataProof?: ByteLike } = {},
  ): Promise<Buffer> {
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

  signPayload(
    payload: unknown,
    context: { proof?: ByteLike; metadataProof?: ByteLike } = {},
  ): Promise<Buffer> {
    if (!(Buffer.isBuffer(payload) || payload instanceof Uint8Array)) {
      throw new Error('LedgerSigner.signPayload does not accept structured payloads; use signBytes')
    }
    return this.signBytes(payload, context)
  }
}

export class LedgerDevice {
  private readonly handle: NativeLedgerHandle

  private constructor(handle: NativeLedgerHandle) {
    if (handle == null) {
      throw new TypeError('Use await LedgerDevice.open()')
    }
    this.handle = handle
  }

  static async open(): Promise<LedgerDevice> {
    return new LedgerDevice(await nativeAsync(() => native.NativeLedgerDevice.open()))
  }

  appVersion(): Promise<LedgerVersion> {
    return nativeAsync(() => this.handle.appVersion())
  }

  async app_version(): Promise<[number, number, number]> {
    const version = await this.appVersion()
    return [version.major, version.minor, version.patch]
  }

  address(
    account = 0,
    index = 0,
    ss58Prefix = 42,
    confirm = false,
  ): Promise<LedgerAddress> {
    return nativeAsync(() => this.handle.address(account, index, ss58Prefix, confirm))
  }

  signer(options: LedgerSignerOptions = {}): LedgerSigner {
    return new LedgerSigner(this, options)
  }

  sign(payload: ByteLike, proof: ByteLike, account?: number, index?: number): Promise<Buffer>
  sign(account: number, index: number, payload: ByteLike, proof: ByteLike): Promise<Buffer>
  sign(
    first: number | ByteLike,
    second: number | ByteLike,
    third?: number | ByteLike,
    fourth?: number | ByteLike,
  ): Promise<Buffer> {
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
    return nativeAsync(() =>
      this.handle.sign(
        account,
        index,
        toBuffer(payload, 'payload'),
        toBuffer(proof, 'proof'),
      ),
    )
  }
}
