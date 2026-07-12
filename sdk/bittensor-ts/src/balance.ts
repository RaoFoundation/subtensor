export const RAO_PER_TAO = 1_000_000_000n
const MAX_SAFE_RAO = BigInt(Number.MAX_SAFE_INTEGER)
const AMOUNT_UNIT = '__bittensorAmountUnit'

export type BalanceLike = Balance | bigint | number | string
export type AmountUnit = 'rao' | 'tao' | 'alpha'
export type TransactionAmount = Balance | bigint | RaoAmount | TaoAmount | AlphaAmount
export type AssetId = bigint | number | string

export interface RaoAmount {
  readonly [AMOUNT_UNIT]: 'rao'
  readonly rao: bigint
}

export interface TaoAmount {
  readonly [AMOUNT_UNIT]: 'tao'
  readonly rao: bigint
}

export interface AlphaAmount {
  readonly [AMOUNT_UNIT]: 'alpha'
  readonly netuid: number
  readonly rao: bigint
}

export class UnitMismatchError extends Error {
  constructor(message: string) {
    super(message)
    this.name = 'UnitMismatchError'
  }
}

export class Balance {
  readonly rao: bigint
  readonly netuid: number
  readonly symbol?: string

  constructor(
    rao: BalanceLike,
    netuid = rao instanceof Balance ? rao.netuid : 0,
    symbol: string | null | undefined = rao instanceof Balance ? rao.symbol : undefined,
  ) {
    this.rao = balanceRao(rao)
    this.netuid = netuid
    this.symbol = symbol ?? undefined
  }

  static fromRao(rao: BalanceLike, netuid = 0, symbol?: string | null): Balance {
    return new Balance(rao, netuid, symbol)
  }

  static fromTao(tao: number | string): Balance {
    return Balance.fromAmount(tao, 0)
  }

  static fromAlpha(alpha: number | string, netuid: number, symbol?: string | null): Balance {
    if (netuid === 0) {
      throw new UnitMismatchError('fromAlpha requires a non-zero netuid; use fromTao for TAO')
    }
    return Balance.fromAmount(alpha, netuid, symbol)
  }

  static fromAmount(amount: number | string, netuid = 0, symbol?: string | null): Balance {
    if (typeof amount === 'number' && !Number.isFinite(amount)) {
      throw new RangeError('balance amount must be finite')
    }
    const text = String(amount).trim()
    if (!/^-?\d+(\.\d+)?$/.test(text)) throw new RangeError(`invalid balance amount ${text}`)
    const negative = text.startsWith('-')
    const unsigned = negative ? text.slice(1) : text
    const [whole, fraction = ''] = unsigned.split('.', 2)
    if (fraction.length > 9) {
      throw new RangeError('balance amount cannot have more than 9 decimal places')
    }
    const padded = fraction.padEnd(9, '0')
    const rao = BigInt(whole || '0') * RAO_PER_TAO + BigInt(padded)
    return new Balance(negative ? -rao : rao, netuid, symbol)
  }

  get tao(): number {
    if (this.netuid !== 0) throw new UnitMismatchError(`This balance is subnet-${this.netuid} alpha, not TAO`)
    return this.amount
  }

  get taoString(): string {
    if (this.netuid !== 0) throw new UnitMismatchError(`This balance is subnet-${this.netuid} alpha, not TAO`)
    return this.amountString
  }

  get alpha(): number {
    if (this.netuid === 0) throw new UnitMismatchError('This balance is TAO, not alpha')
    return this.amount
  }

  get alphaString(): string {
    if (this.netuid === 0) throw new UnitMismatchError('This balance is TAO, not alpha')
    return this.amountString
  }

  get amount(): number {
    return this.toNumber()
  }

  get amountString(): string {
    return formatRao(this.rao)
  }

  get unit(): string {
    if (this.netuid === 0) return this.symbol ?? 'TAO'
    return this.symbol ?? `alpha${this.netuid}`
  }

  withSymbol(symbol: string | null): Balance {
    return new Balance(this.rao, this.netuid, symbol)
  }

  add(other: BalanceLike): Balance {
    return new Balance(this.rao + this.raoOf(other), this.netuid, this.symbol)
  }

  sub(other: BalanceLike): Balance {
    return new Balance(this.rao - this.raoOf(other), this.netuid, this.symbol)
  }

  neg(): Balance {
    return new Balance(-this.rao, this.netuid, this.symbol)
  }

  eq(other: BalanceLike): boolean {
    return this.rao === this.raoOf(other)
  }

  toJSON(): string {
    return this.rao.toString()
  }

  toString(): string {
    return `${this.amountString} ${this.unit}`
  }

  toNumber(): number {
    if (abs(this.rao) > MAX_SAFE_RAO) {
      throw new RangeError('balance exceeds JavaScript safe integer precision; use rao or amountString')
    }
    return Number(this.rao) / Number(RAO_PER_TAO)
  }

  private raoOf(other: BalanceLike): bigint {
    if (other instanceof Balance) {
      if (other.netuid !== this.netuid) {
        throw new UnitMismatchError(`Cannot combine netuid ${this.netuid} with netuid ${other.netuid}`)
      }
      return other.rao
    }
    return balanceRao(other)
  }
}

function abs(value: bigint): bigint {
  return value < 0n ? -value : value
}

function formatRao(rao: bigint): string {
  const sign = rao < 0n ? '-' : ''
  const value = rao < 0n ? -rao : rao
  const whole = value / RAO_PER_TAO
  const fraction = (value % RAO_PER_TAO).toString().padStart(9, '0').replace(/0+$/, '')
  return `${sign}${whole.toString()}${fraction ? `.${fraction}` : ''}`
}

export function balanceRao(value: BalanceLike): bigint {
  if (value instanceof Balance) return value.rao
  return parseRao(value)
}

export function transactionAmountRao(
  value: TransactionAmount,
  options: { name?: string; taoOnly?: boolean; alphaOnly?: boolean } = {},
): bigint {
  const name = options.name ?? 'transaction amount'
  let rao: bigint
  if (value instanceof Balance) {
    if (options.taoOnly && value.netuid !== 0) {
      throw new UnitMismatchError(`${name} must be a TAO balance, not subnet-${value.netuid} alpha`)
    }
    if (options.alphaOnly && value.netuid === 0) {
      throw new UnitMismatchError(`${name} must be an alpha balance, not TAO`)
    }
    rao = value.rao
  } else if (typeof value === 'bigint') {
    rao = value
  } else if (isBrandedAmount(value)) {
    const unit = brandedAmountUnit(value)
    if (options.taoOnly && unit === 'alpha') {
      throw new UnitMismatchError(`${name} must be a TAO amount, not subnet-${brandedAmountNetuid(value) ?? 'unknown'} alpha`)
    }
    if (options.alphaOnly && unit === 'tao') {
      throw new UnitMismatchError(`${name} must be an alpha amount, not TAO`)
    }
    rao = value.rao
  } else {
    throw new TypeError(
      `${name} must be a Balance, bigint rao amount, raoAmount(...), taoAmount(...), or alphaAmount(...)`,
    )
  }
  if (rao < 0n) throw new RangeError(`${name} must be non-negative`)
  return rao
}

export function assetIdValue(value: AssetId, name = 'asset ID'): bigint {
  const parsed = parseInteger(value, name)
  if (parsed < 0n) throw new RangeError(`${name} must be non-negative`)
  return parsed
}

export function asset_id_value(value: AssetId, name = 'asset ID'): bigint {
  return assetIdValue(value, name)
}

export function taoTransactionAmountRao(value: TransactionAmount, name = 'transaction amount'): bigint {
  return transactionAmountRao(value, { name, taoOnly: true })
}

export function tao_transaction_amount_rao(value: TransactionAmount, name = 'transaction amount'): bigint {
  return taoTransactionAmountRao(value, name)
}

export function alphaTransactionAmountRao(value: TransactionAmount, name = 'transaction amount'): bigint {
  return transactionAmountRao(value, { name, alphaOnly: true })
}

export function alpha_transaction_amount_rao(value: TransactionAmount, name = 'transaction amount'): bigint {
  return alphaTransactionAmountRao(value, name)
}

export function raoAmount(value: bigint | number | string): RaoAmount {
  return Object.freeze({
    [AMOUNT_UNIT]: 'rao',
    rao: parseRao(value),
  }) as RaoAmount
}

export function taoAmount(value: number | string): TaoAmount {
  return Object.freeze({
    [AMOUNT_UNIT]: 'tao',
    rao: Balance.fromTao(value).rao,
  }) as TaoAmount
}

export function alphaAmount(value: number | string, netuid: number): AlphaAmount {
  const normalizedNetuid = alphaAmountNetuid(netuid)
  return Object.freeze({
    [AMOUNT_UNIT]: 'alpha',
    netuid: normalizedNetuid,
    rao: Balance.fromAlpha(value, normalizedNetuid).rao,
  }) as AlphaAmount
}

function parseRao(value: bigint | number | string): bigint {
  return parseInteger(value, 'balance rao')
}

function parseInteger(value: bigint | number | string, name: string): bigint {
  if (typeof value === 'bigint') return value
  if (typeof value === 'number') {
    if (!Number.isSafeInteger(value)) throw new RangeError(`${name} must be a safe integer`)
    return BigInt(value)
  }
  if (typeof value !== 'string') {
    throw new TypeError(`${name} must be a bigint, safe integer number, or integer string`)
  }
  const text = value.trim()
  if (/^-?\d+$/.test(text)) return BigInt(text)
  throw new RangeError(`${name} must be an integer`)
}

function alphaAmountNetuid(netuid: number): number {
  if (!Number.isSafeInteger(netuid) || netuid <= 0) {
    throw new RangeError('alphaAmount requires a positive safe-integer netuid')
  }
  return netuid
}

export function brandedAmountUnit(value: unknown): AmountUnit | undefined {
  if (typeof value !== 'object' || value == null) return undefined
  if (typeof (value as { rao?: unknown }).rao !== 'bigint') return undefined
  const unit = (value as Record<typeof AMOUNT_UNIT, unknown>)[AMOUNT_UNIT]
  return unit === 'rao' || unit === 'tao' || unit === 'alpha' ? unit : undefined
}

export function brandedAmountNetuid(value: unknown): number | undefined {
  if (brandedAmountUnit(value) !== 'alpha') return undefined
  const netuid = (value as { netuid?: unknown }).netuid
  return Number.isSafeInteger(netuid) && Number(netuid) > 0 ? Number(netuid) : undefined
}

function isBrandedAmount(value: unknown): value is RaoAmount | TaoAmount | AlphaAmount {
  return brandedAmountUnit(value) != null
}

export const tao = Balance.fromTao
export const alpha = Balance.fromAlpha
export const rao = Balance.fromRao
export const rao_amount = raoAmount
export const tao_amount = taoAmount
export const alpha_amount = alphaAmount
export const transaction_amount_rao = transactionAmountRao
