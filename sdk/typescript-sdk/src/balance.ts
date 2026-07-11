export const RAO_PER_TAO = 1_000_000_000n

export type BalanceLike = Balance | bigint | number | string

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
    const padded = `${fraction}000000000`.slice(0, 9)
    const rao = BigInt(whole || '0') * RAO_PER_TAO + BigInt(padded)
    return new Balance(negative ? -rao : rao, netuid, symbol)
  }

  get tao(): number {
    if (this.netuid !== 0) throw new UnitMismatchError(`This balance is subnet-${this.netuid} alpha, not TAO`)
    return this.amount
  }

  get alpha(): number {
    if (this.netuid === 0) throw new UnitMismatchError('This balance is TAO, not alpha')
    return this.amount
  }

  get amount(): number {
    return Number(this.rao) / Number(RAO_PER_TAO)
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
    const sign = this.rao < 0n ? '-' : ''
    const value = this.rao < 0n ? -this.rao : this.rao
    const whole = value / RAO_PER_TAO
    const fraction = (value % RAO_PER_TAO).toString().padStart(9, '0').replace(/0+$/, '')
    return `${sign}${whole.toString()}${fraction ? `.${fraction}` : ''} ${this.unit}`
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

export function balanceRao(value: BalanceLike): bigint {
  if (value instanceof Balance) return value.rao
  if (typeof value === 'bigint') return value
  if (typeof value === 'number') {
    if (!Number.isSafeInteger(value)) throw new RangeError('balance rao must be a safe integer')
    return BigInt(value)
  }
  if (/^-?\d+$/.test(value)) return BigInt(value)
  return Balance.fromTao(value).rao
}

export const tao = Balance.fromTao
export const alpha = Balance.fromAlpha
export const rao = Balance.fromRao
