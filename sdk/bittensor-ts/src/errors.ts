export type BittensorCoreErrorCode =
  | 'KEYFILE'
  | 'WRONG_PASSWORD'
  | 'NOT_IN_RUNTIME'
  | 'CODEC'
  | 'CRYPTO'
  | 'DEVICE'
  | 'UNKNOWN'

export class BittensorCoreError extends Error {
  readonly code: BittensorCoreErrorCode

  constructor(code: BittensorCoreErrorCode, message: string, options?: ErrorOptions) {
    super(message, options)
    this.name = 'BittensorCoreError'
    this.code = code
  }
}

export class KeyfileError extends BittensorCoreError {
  constructor(message: string, options?: ErrorOptions) {
    super('KEYFILE', message, options)
    this.name = 'KeyfileError'
  }
}

export class WrongPasswordError extends BittensorCoreError {
  constructor(message: string, options?: ErrorOptions) {
    super('WRONG_PASSWORD', message, options)
    this.name = 'WrongPasswordError'
  }
}

export class NotInRuntimeError extends BittensorCoreError {
  constructor(message: string, options?: ErrorOptions) {
    super('NOT_IN_RUNTIME', message, options)
    this.name = 'NotInRuntimeError'
  }
}

export class CodecError extends BittensorCoreError {
  constructor(message: string, options?: ErrorOptions) {
    super('CODEC', message, options)
    this.name = 'CodecError'
  }
}

export class CryptoError extends BittensorCoreError {
  constructor(message: string, options?: ErrorOptions) {
    super('CRYPTO', message, options)
    this.name = 'CryptoError'
  }
}

export class DeviceError extends BittensorCoreError {
  constructor(message: string, options?: ErrorOptions) {
    super('DEVICE', message, options)
    this.name = 'DeviceError'
  }
}

const MARKER = /\[BITTENSOR_CORE:(KEYFILE|WRONG_PASSWORD|NOT_IN_RUNTIME|CODEC|CRYPTO|DEVICE)\]\s*/

export function mapNativeError(error: unknown): Error {
  if (error instanceof BittensorCoreError) return error
  if (!(error instanceof Error)) return new BittensorCoreError('UNKNOWN', String(error))

  const match = MARKER.exec(error.message)
  if (!match) return error
  const code = match[1] as Exclude<BittensorCoreErrorCode, 'UNKNOWN'>
  const message = error.message.replace(MARKER, '')
  const options = { cause: error }
  switch (code) {
    case 'KEYFILE':
      return new KeyfileError(message, options)
    case 'WRONG_PASSWORD':
      return new WrongPasswordError(message, options)
    case 'NOT_IN_RUNTIME':
      return new NotInRuntimeError(message, options)
    case 'CODEC':
      return new CodecError(message, options)
    case 'CRYPTO':
      return new CryptoError(message, options)
    case 'DEVICE':
      return new DeviceError(message, options)
  }
}

export function nativeCall<T>(operation: () => T): T {
  try {
    const value = operation()
    if (value instanceof Error) throw value
    return value
  } catch (error) {
    throw mapNativeError(error)
  }
}

export { BittensorCoreError as CoreError }
