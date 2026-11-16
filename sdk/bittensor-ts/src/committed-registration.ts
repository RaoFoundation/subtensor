import type { IntentDescriptor, IntentReader } from './intents'
import type { RegistrationSponsorship } from './registration-sponsorship'
import { quoteRegistrationDeposit } from './registration-deposit'
import { evaluatePolicy } from './policy'
import { hexBytes } from './portable'

/** Restore consent and affordability metadata from the exact committed calls. */
export async function committedRegistrationSponsorships(call: IntentDescriptor, reader: IntentReader, payer: string): Promise<{
  sponsorships: RegistrationSponsorship[]
  reserves: Array<{ address: string; requiredRao: bigint; availableRao: bigint }>
}> {
  const rows = evaluatePolicy<Array<{ descriptor: string; payer: string; maximum: string | null }>>('financial.registration.committed_calls', { call, payer })
  const sponsorships: RegistrationSponsorship[] = [], reserves: Array<{ address: string; requiredRao: bigint; availableRao: bigint }> = []
  if (!rows.length) return { sponsorships, reserves }
  if (!reader.descriptorAddress) throw new Error('committed registration descriptor identity is unavailable')
  const block = await (reader.blockHash?.() ?? reader.finalizedHead?.())
  if (!block) throw new Error('pinned registration deposit metadata is required')
  for (const row of rows) {
    const descriptor = hexBytes(row.descriptor)
    const recipient = reader.descriptorAddress(descriptor)
    const quote = await quoteRegistrationDeposit(reader, descriptor[0] === 2 ? 'MlDsaRegistrationDeposit' : 'RegistrationDeposit', block)
    const depositRao = BigInt(evaluatePolicy<string>('financial.registration.committed_quote', { current: quote.amountRao, configurable: quote.configurable, maximum: row.maximum }))
    sponsorships.push({ payer: row.payer, recipient, depositRao })
    if (depositRao > 0n) {
      const account = await reader.query('System', 'Account', [row.payer], block) as { data?: { free?: unknown } } | null
      const availableRao = BigInt(evaluatePolicy<string>('financial.registration.deposit', { value: account?.data?.free ?? 0 }))
      reserves.push({ address: row.payer, requiredRao: depositRao, availableRao })
    }
  }
  return { sponsorships, reserves }
}
