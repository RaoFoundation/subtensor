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

  sign(
    account: number,
    index: number,
    payload: ByteLike,
    proof: ByteLike,
  ): Buffer {
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
