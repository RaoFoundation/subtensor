import { bytesHex, hexBytes, concatBytes, type PortableCrypto } from './portable'

export interface UnsignedTransaction {
  address: string; publicKey: Uint8Array; cryptoType: number; callData: Uint8Array
  genesisHash: string; payload: Uint8Array; includedInExtrinsic: Uint8Array; includedInSignedData: Uint8Array
  signerPayload: Record<string, unknown>; metadataProof?: Uint8Array
  [key: string]: unknown
}
export const UOS_FRAME_SIZE = 1024
function compact(value: number): Uint8Array {
  if (!Number.isSafeInteger(value) || value < 0 || value >= 2 ** 30) throw new RangeError('call exceeds compact-u32 framing')
  const size = value < 64 ? 1 : value < 16384 ? 2 : 4
  const encoded = value * 4 + (size === 1 ? 0 : size === 2 ? 1 : 2)
  return Uint8Array.from({ length: size }, (_, i) => Math.floor(encoded / 2 ** (i * 8)) & 255)
}
export function uosSignFrame(cryptoType: number, command: number, publicKey: Uint8Array, payload: Uint8Array, genesisHash: Uint8Array): Uint8Array {
  if (![0, 1].includes(cryptoType) || publicKey.length !== 32 || genesisHash.length !== 32) throw new TypeError('invalid UOS signing identity')
  return concatBytes(new Uint8Array([0x53, cryptoType, command]), publicKey, payload, genesisHash)
}
export function uosMultipartFrames(data: Uint8Array): Uint8Array[] {
  const count = Math.max(1, Math.ceil(data.length / UOS_FRAME_SIZE))
  if (count > 65535) throw new RangeError('too many UOS frames')
  return Array.from({ length: count }, (_, index) => concatBytes(new Uint8Array([0, count >> 8, count & 255, index >> 8, index & 255]), data.subarray(index * UOS_FRAME_SIZE, (index + 1) * UOS_FRAME_SIZE)))
}
export function vaultTransactionFrames(unsigned: UnsignedTransaction, proof = unsigned.metadataProof): { frames: Uint8Array[]; blind: boolean } {
  const blind = proof == null && unsigned.callData.length * 2 + 2 > 5000
  if (blind && unsigned.payload.length !== 32) throw new TypeError('oversized call needs a hashed signing payload')
  const payload = blind ? unsigned.payload : concatBytes(proof ?? new Uint8Array(), compact(unsigned.callData.length), unsigned.callData, unsigned.includedInExtrinsic, unsigned.includedInSignedData)
  return { frames: uosMultipartFrames(uosSignFrame(unsigned.cryptoType, proof ? 6 : blind ? 1 : 2, unsigned.publicKey, payload, hexBytes(unsigned.genesisHash))), blind }
}
export type VaultSignatureRequest = (request: { frames: Uint8Array[]; blind: boolean; unsigned?: UnsignedTransaction }) => Promise<Uint8Array | string>
export class VaultSigner {
  readonly requiresMetadataProof = true
  readonly requiresMetadataHash = true
  constructor(readonly ss58Address: string, readonly publicKey: Uint8Array, readonly requestSignature: VaultSignatureRequest, readonly cryptoType = 1, private readonly crypto?: PortableCrypto) {
    if (publicKey.length !== 32 || ![0, 1].includes(cryptoType)) throw new TypeError('invalid Vault identity')
  }
  async signUnsignedExtrinsic(unsigned: UnsignedTransaction): Promise<Uint8Array> {
    if (!this.crypto) throw new Error('Vault signing requires a signature verification backend')
    if (unsigned.address !== this.ss58Address || bytesHex(unsigned.publicKey) !== bytesHex(this.publicKey) || unsigned.cryptoType !== this.cryptoType) throw new Error('prepared transaction belongs to a different signer')
    const response = await this.requestSignature({ ...vaultTransactionFrames(unsigned), unsigned })
    const signature = typeof response === 'string' ? hexBytes(response) : response
    const raw = signature.length === 65 ? signature.subarray(1) : signature
    if (raw.length !== 64 || (signature.length === 65 && signature[0] !== this.cryptoType) || !this.crypto.verify(unsigned.payload, raw, this.ss58Address, this.cryptoType)) throw new Error('invalid Vault signature')
    return new Uint8Array(signature)
  }
  async signPayload(payload: Record<string, unknown>, context?: { callData: Uint8Array; payload: Uint8Array; includedInExtrinsic?: Uint8Array; includedInSignedData?: Uint8Array; metadataProof?: Uint8Array; genesisHash?: string; txParams: { genesisHash: string | Uint8Array } }): Promise<Uint8Array> {
    if (!context?.includedInExtrinsic || !context.includedInSignedData) throw new Error('Vault signing requires prepared transaction context')
    return this.signUnsignedExtrinsic({ address: this.ss58Address, publicKey: this.publicKey, cryptoType: this.cryptoType, callData: context.callData, payload: context.payload, genesisHash: context.genesisHash ?? (typeof context.txParams.genesisHash === 'string' ? context.txParams.genesisHash : bytesHex(context.txParams.genesisHash)), includedInExtrinsic: context.includedInExtrinsic, includedInSignedData: context.includedInSignedData, metadataProof: context.metadataProof, signerPayload: payload })
  }
}

export interface ExtensionAccount { address: string; name?: string; source?: string; type?: string; genesisHash?: string | null }
export interface InjectedExtension {
  accounts: { get(): Promise<ExtensionAccount[]>; subscribe?(callback: (accounts: ExtensionAccount[]) => void): Promise<() => void> }
  signer: { signPayload?(payload: Record<string, unknown>): Promise<{ signature: string }>; signRaw?(payload: { address: string; data: string; type: 'bytes' }): Promise<{ signature: string }> }
}
export interface ExtensionProvider { enable(appName: string): Promise<InjectedExtension> }
export class ExtensionSigner {
  readonly address: string
  readonly ss58Address: string
  readonly cryptoType: number
  constructor(readonly account: ExtensionAccount, readonly extension: InjectedExtension) {
    this.address = this.ss58Address = account.address
    this.cryptoType = account.type == null || account.type === 'sr25519' ? 1 : account.type === 'ed25519' ? 0 : -1
    if (this.cryptoType < 0) throw new TypeError(`unsupported extension account type ${account.type}`)
  }
  signPayload(payload: Record<string, unknown>): Promise<{ signature: string }> {
    if (payload.address !== this.address) throw new Error('extension signer address mismatch')
    if (this.account.genesisHash && String(payload.genesisHash).toLowerCase() !== this.account.genesisHash.toLowerCase()) throw new Error('extension account belongs to another chain')
    if (!this.extension.signer.signPayload) throw new Error('extension does not support transaction signing')
    return this.extension.signer.signPayload(payload)
  }
  signRaw(payload: { address: string; data: string; type: 'bytes' }): Promise<{ signature: string }> {
    if (payload.address !== this.address) throw new Error('extension signer address mismatch')
    if (!this.extension.signer.signRaw) throw new Error('extension does not support message signing')
    return this.extension.signer.signRaw(payload)
  }
  async sign(payload: Uint8Array): Promise<Uint8Array> {
    const signature = hexBytes((await this.signRaw({ address: this.address, data: bytesHex(payload), type: 'bytes' })).signature)
    if (signature.length === 65) {
      if (signature[0] !== this.cryptoType) throw new Error('extension signature scheme mismatch')
      return signature.slice(1)
    }
    if (signature.length !== 64) throw new Error('invalid extension signature length')
    return signature
  }
}
export async function extensionAccounts(appName: string, providers: Record<string, ExtensionProvider> = (globalThis as typeof globalThis & { injectedWeb3?: Record<string, ExtensionProvider> }).injectedWeb3 ?? {}): Promise<Array<{ account: ExtensionAccount; extension: InjectedExtension }>> {
  const result: Array<{ account: ExtensionAccount; extension: InjectedExtension }> = []
  for (const [source, provider] of Object.entries(providers)) {
    const extension = await provider.enable(appName)
    for (const account of await extension.accounts.get()) result.push({ account: { ...account, source }, extension })
  }
  return result
}
export async function connectExtensionSigner(appName: string, options: { address?: string; source?: string; select?: (accounts: ExtensionAccount[]) => ExtensionAccount | Promise<ExtensionAccount>; providers?: Record<string, ExtensionProvider> } = {}): Promise<ExtensionSigner> {
  const choices = (await extensionAccounts(appName, options.providers)).filter(({ account }) => (!options.address || account.address === options.address) && (!options.source || account.source === options.source))
  const selected = options.select ? await options.select(choices.map(item => item.account)) : choices.length === 1 ? choices[0].account : undefined
  const found = selected && choices.find(({ account }) => account.address === selected.address && account.source === selected.source)
  if (!found) throw new Error(choices.length ? 'select an explicit extension account' : 'no matching extension account')
  return new ExtensionSigner(found.account, found.extension)
}
