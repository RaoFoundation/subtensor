import type { PreparedIntent } from './intents'
import { u64 } from './wallet-types'

export interface RegistrationSponsorship { payer: string; recipient: string; depositRao: bigint }
export interface RegistrationSponsorshipOptions {
  /** Total native TAO reserve allowance in rao, separate from payment and fees. */
  maxRegistrationDepositRao?: bigint | number
  onRegistrationSponsorship?: (quote: RegistrationSponsorship) => boolean | Promise<boolean>
}
export async function authorizeRegistrationSponsorship(plan: PreparedIntent, options: RegistrationSponsorshipOptions): Promise<void> {
  const maximum = options.maxRegistrationDepositRao == null ? undefined : u64(options.maxRegistrationDepositRao)
  const quotes = (plan.extras.registrationSponsorships ?? []) as RegistrationSponsorship[]
  const total = quotes.reduce((amount,quote) => amount + u64(quote.depositRao),0n)
  if (!total) return
  if (maximum != null && total > maximum) throw new Error('registration reserve exceeds maxRegistrationDepositRao')
  if (maximum == null && !options.onRegistrationSponsorship) throw new Error('recipient registration requires maxRegistrationDepositRao or onRegistrationSponsorship')
  if (options.onRegistrationSponsorship) for (const quote of quotes) if (await options.onRegistrationSponsorship(Object.freeze({...quote})) !== true) throw new Error('registration sponsorship declined')
}
