import { policyEvaluate as bindingEvaluate } from '#bittensor-policy'

export function policyEvaluate(operation: string, input: string): string {
  return bindingEvaluate(operation, input)
}

/** Structural policies may rearrange calls without converting their codec values. */
export function evaluatePolicyPreservingValues<T = unknown>(operation: string, input: unknown): T {
  const strings = new Set<string>()
  const visit = (value: unknown, depth: number): void => {
    if (depth > 128) throw new RangeError('policy value nesting exceeds 128')
    if (typeof value === 'string') strings.add(value)
    else if (Array.isArray(value)) value.forEach(child => visit(child, depth + 1))
    else if (value && typeof value === 'object' && !(value instanceof Uint8Array)) {
      for (const [key, child] of Object.entries(value)) {
        strings.add(key)
        visit(child, depth + 1)
      }
    }
  }
  visit(input, 0)
  let prefix = '__bittensor_policy_value__'
  while ([...strings].some(value => value.startsWith(prefix))) prefix += '_'
  const scalars = new Map<string, bigint | Uint8Array>()
  const encode = (value: any): any => {
    if (typeof value === 'bigint' || value instanceof Uint8Array) {
      const token = prefix + scalars.size
      scalars.set(token, value)
      return token
    }
    if (Array.isArray(value)) return value.map(encode)
    if (value && typeof value === 'object') return Object.fromEntries(Object.entries(value).map(([key, child]) => [key, encode(child)]))
    return value
  }
  const decode = (value: any): any => {
    if (typeof value === 'string' && scalars.has(value)) return scalars.get(value)
    if (Array.isArray(value)) return value.map(decode)
    if (value && typeof value === 'object') return Object.fromEntries(Object.entries(value).map(([key, child]) => [key, decode(child)]))
    return value
  }
  return decode(evaluatePolicy(operation, encode(input))) as T
}

/** Lossless language adaptation only; all shared decisions run in Rust. */
export function evaluatePolicy<T = unknown>(operation: string, input: unknown): T {
  const encoded = JSON.stringify(input, function (key, value) {
    const original = key === '' ? input : this[key]
    if (typeof value === 'bigint') return value.toString()
    if (original instanceof Uint8Array) return '0x' + Array.from(original, byte => byte.toString(16).padStart(2, '0')).join('')
    if (typeof value === 'number' && !Number.isFinite(value)) throw new RangeError('policy numbers must be finite')
    return value
  })
  const response = JSON.parse(policyEvaluate(operation, encoded ?? 'null'))
  if (response.version !== 1) throw new Error('incompatible Rust SDK policy version')
  if (response.error) {
    const ErrorClass = response.error.kind === 'type' ? TypeError : response.error.kind === 'range' ? RangeError : Error
    throw new ErrorClass(response.error.message)
  }
  return response.value as T
}
