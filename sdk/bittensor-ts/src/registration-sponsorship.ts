import type { PreparedIntent } from './intents'
import { evaluatePolicy } from './policy'

export interface RegistrationSponsorship { payer: string; recipient: string; depositRao: bigint }
export interface RegistrationSponsorshipOptions {
  /** Total native TAO reserve allowance in rao, separate from payment and fees. */
  maxRegistrationDepositRao?: bigint | number
  onRegistrationSponsorship?: (quote: RegistrationSponsorship) => boolean | Promise<boolean>
}
export async function authorizeRegistrationSponsorship(plan: PreparedIntent, options: RegistrationSponsorshipOptions): Promise<void> {
  const quotes = (plan.extras.registrationSponsorships ?? []) as RegistrationSponsorship[]
  const policy = evaluatePolicy<{ required: boolean; reason: 'maximum' | 'consent' | null }>('financial.registration.sponsorship', { deposits: quotes.map(quote => quote.depositRao), maximum: options.maxRegistrationDepositRao, callback: Boolean(options.onRegistrationSponsorship) })
  if (!policy.required) return
  if (policy.reason === 'maximum') throw new Error('registration reserve exceeds maxRegistrationDepositRao')
  if (policy.reason === 'consent') throw new Error('recipient registration requires maxRegistrationDepositRao or onRegistrationSponsorship')
  if (options.onRegistrationSponsorship) for (const quote of quotes) if (await options.onRegistrationSponsorship(Object.freeze({...quote})) !== true) throw new Error('registration sponsorship declined')
}
