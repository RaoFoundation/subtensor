import { evaluatePolicy } from './policy'
import type { IntentDescriptor, IntentReader } from './intents'

export interface RegistrationDepositQuote { amountRao: bigint; configurable: boolean }

/** Read the reserve and its metadata at one block; never turn an RPC failure into a legacy quote. */
export async function quoteRegistrationDeposit(reader: IntentReader, legacyConstant: string, at?: string): Promise<RegistrationDepositQuote> {
  const block = at ?? await (reader.blockHash?.() ?? reader.finalizedHead?.())
  if (!block || !reader.hasStorage) throw new Error('pinned registration deposit metadata is required')
  const configurable = await reader.hasStorage('HashedAccounts', 'HashedAccountRegistrationDeposit', block)
  const value = configurable
    ? await reader.query('HashedAccounts', 'HashedAccountRegistrationDeposit', [], block)
    : await reader.constant?.('HashedAccounts', legacyConstant, block)
  const amountRao = BigInt(evaluatePolicy<string>('financial.registration.deposit', { value }))
  return { amountRao, configurable }
}

export function registrationDepositGuard(quote: RegistrationDepositQuote): IntentDescriptor {
  return { pallet: 'HashedAccounts', call: 'check_registration_deposit', params: { max_deposit: quote.amountRao } }
}
