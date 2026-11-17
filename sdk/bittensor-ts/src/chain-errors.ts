import {
  ErrorCode as ErrorCodes, CHAIN_ERROR_CODES, CUSTOM_TRANSACTION_ERRORS, INVALID_TRANSACTION_ERRORS,
  DISPATCH_ERROR_CODES, CHAIN_ERROR_CATALOG, ERROR_SUBSTRING_FALLBACK,
} from './error-catalog'

export { CHAIN_ERROR_CATALOG }
export const ErrorCode = ErrorCodes
export type ErrorCode = typeof ErrorCode[keyof typeof ErrorCode]

/** Stable, JSON-safe failure details for application error handling. */
export interface ChainErrorInfo {
  readonly code: ErrorCode
  readonly message: string
  readonly name?: string
  readonly pallet?: string
  readonly docs?: string
  readonly remediation: string
}

export const ERROR_REMEDIATION: Readonly<Record<ErrorCode, string>> = Object.freeze({
  insufficient_balance: 'Fund the paying account or reduce the amount, accounting for locks, deposits, fees, and tips.',
  insufficient_liquidity: 'Reduce the trade amount or wait for sufficient pool liquidity.',
  rate_limited: 'Wait for the applicable rate limit before retrying.',
  not_registered: 'Register the hotkey on the target subnet first.',
  not_authorized: 'Use the key or dispatch origin authorized for this operation.',
  already_exists: 'Read the existing state before deciding whether another operation is needed.',
  not_found: 'Check the identifier and read the referenced on-chain object.',
  subnet_not_exists: 'Use a subnet that exists on this chain.',
  subtoken_disabled: 'Wait for the subnet to activate.',
  disabled: 'Check whether the operation is enabled on this network.',
  too_early: 'Wait for the required block, epoch, or transaction nonce.',
  expired: 'Prepare and sign again using fresh chain state.',
  limit_exceeded: 'Reduce the size, count, or amount to fit the applicable chain limit.',
  unit_mismatch: 'Match the Balance netuid to the operation currency.',
  invalid_argument: 'Check the arguments against the operation schema.',
  policy_violation: 'Review the operation against the configured policy.',
  internal: 'Retain the block hash and failure details for diagnosis.',
  unknown: 'Inspect the original failure and chain state before retrying.',
})

const names: Readonly<Record<string, ErrorCode>> = CHAIN_ERROR_CODES
const dispatch: Readonly<Record<string, ErrorCode>> = DISPATCH_ERROR_CODES
const custom: Readonly<Record<string, string>> = CUSTOM_TRANSACTION_ERRORS
const watch: Readonly<Record<string, string>> = { usurped: 'Usurped', retracted: 'Retracted', finalitytimeout: 'FinalityTimeout', dropped: 'Dropped', invalid: 'Invalid' }
const record = (value: unknown): Record<string, unknown> | undefined => value != null && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : undefined

function poolName(message: string, payload?: unknown): string | undefined {
  const data = record(payload)
  const nested = record(data?.error)
  const rpcData = nested?.data ?? data?.data ?? payload
  const code = record(rpcData)?.Custom ?? (typeof rpcData === 'number' ? rpcData : undefined)
  const match = /(?:custom error:\s*|['"]Custom['"]\s*:\s*)(\d+)/i.exec(message + ' ' + (typeof rpcData === 'string' ? rpcData : ''))
  const number = code ?? match?.[1]
  if (number != null && Object.hasOwn(custom, String(number))) return custom[String(number)]
  const status = /\bextrinsic\s+(usurped|retracted|finalitytimeout|dropped|invalid)\b/i.exec(message)
  if (status) return watch[status[1].toLowerCase()]
  return INVALID_TRANSACTION_ERRORS.find(([needle]) => message.toLowerCase().includes(needle))?.[1]
}

/** Exact names take precedence; unknown named failures never inherit a guess from their text. */
export function classifyError(text: string, name?: string): ErrorCode {
  if (name) {
    if (Object.hasOwn(names, name)) return names[name]
    if (Object.hasOwn(dispatch, name)) return dispatch[name]
    return name.includes('RateLimit') ? ErrorCode.RATE_LIMITED : ErrorCode.UNKNOWN
  }
  return ERROR_SUBSTRING_FALLBACK.find(([needle]) => text.toLowerCase().includes(needle))?.[1] ?? ErrorCode.UNKNOWN
}

/** Classify decoded names, receipt messages, or Substrate RPC failures. Custom codes are not Ethereum codes. */
export function classifyChainError(error: unknown, options: { name?: string; pallet?: string; docs?: string; payload?: unknown } = {}): ChainErrorInfo {
  const value = record(error)
  const message = error instanceof Error ? error.message : typeof error === 'string' ? error : typeof value?.message === 'string' ? value.message : String(error)
  let name = options.name
  let pallet = options.pallet
  if (!name) name = poolName(message, options.payload ?? value?.details ?? value?.payload ?? (value?.data === undefined ? undefined : { data: value.data }))
  if (!name) {
    const prefix = /^\s*(?:([A-Za-z][A-Za-z0-9_]*)\.)?([A-Za-z][A-Za-z0-9_]*)(?::|$)/.exec(message)
    if (prefix) { name = prefix[2]; pallet ??= prefix[1] }
  }
  const code = classifyError(message, name)
  const catalog = name ? CHAIN_ERROR_CATALOG.find(item => item.name === name && (!pallet || item.pallet === pallet)) : undefined
  return Object.freeze({ code, message, name, pallet, docs: options.docs ?? catalog?.docs, remediation: ERROR_REMEDIATION[code] })
}

export type ModuleErrorResolver = (palletIndex: number, errorIndex: number) => { name: string; docs: readonly string[] } | [string, string[]]

/** Decode using the inclusion block's execution metadata, including errors inside wrappers. */
export function chainErrorFromDispatch(error: unknown, resolveModule?: ModuleErrorResolver): ChainErrorInfo {
  const seen = new Set<object>()
  function decode(value: unknown, depth: number): ChainErrorInfo {
    const data = record(value)
    if (!data || depth > 8 || seen.has(data)) return classifyChainError(typeof value === 'string' ? value : 'Unknown dispatch error')
    seen.add(data)
    if (data.dispatch_error != null) return decode(data.dispatch_error, depth + 1)
    if (data.attributes != null) return decode(data.attributes, depth + 1)
    if (data.Err != null) return decode(data.Err, depth + 1)
    const module = record(data.Module ?? data.module)
    if (module) {
      const index = Number(module.index)
      const raw = module.error
      const errorIndex = typeof raw === 'string' && /^0x[0-9a-f]{2}/i.test(raw) ? parseInt(raw.slice(2, 4), 16) : Number(raw instanceof Uint8Array || Array.isArray(raw) ? raw[0] : raw)
      if (resolveModule && [index, errorIndex].every(n => Number.isInteger(n) && n >= 0 && n <= 255)) {
        try {
          const resolved = resolveModule(index, errorIndex)
          const name = Array.isArray(resolved) ? resolved[0] : resolved.name
          const docs = (Array.isArray(resolved) ? resolved[1] : resolved.docs).join(' ').trim()
          const parts = name.split('.')
          return classifyChainError(docs || name, { name: parts.at(-1), pallet: parts.length > 1 ? parts[0] : undefined, docs })
        } catch { /* Unknown metadata must remain unknown, not use a newer catalog's index. */ }
      }
      return classifyChainError(`Unknown module error ${index}/${errorIndex}`)
    }
    const entries = Object.entries(data)
    if (entries.length === 1) {
      const [name, nested] = entries[0]
      if (['Token', 'Arithmetic', 'Transactional'].includes(name)) return decode(nested, depth + 1)
      return classifyChainError(name, { name })
    }
    return classifyChainError('Unknown dispatch error')
  }
  return decode(error, 0)
}
