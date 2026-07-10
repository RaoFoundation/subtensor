import native, { type NativeLedgerHandle } from './native'
import { nativeCall } from './errors'
import { toBuffer } from './wire'
import type { ByteLike, LedgerAddress, LedgerVersion } from './types'

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
