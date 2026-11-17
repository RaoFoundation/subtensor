import { POLICY_CATALOG } from './policy-catalog'
import { evaluatePolicy } from './policy'
/** Mirrors runtime/src/fee_filters.rs. These signing-hotkey calls charge its owner. */
export const COLDKEY_PAYS_FEE_CALLS: ReadonlySet<string> = new Set(POLICY_CATALOG.financial.COLDKEY_PAYS_FEE_CALLS)
export const COLDKEY_FEE_WARNING = "transaction fee is charged to the hotkey owner's coldkey, not the signing hotkey"

export function* iterCallLeaves(call: unknown): Generator<readonly [string, string]> {
  try { yield* evaluatePolicy<Array<[string, string]>>('financial.fee.leaves', { call }) }
  catch (error) { if (error instanceof TypeError && /circular/i.test(error.message)) throw new TypeError('cyclic call structure'); throw error }
}
export function chargesColdkeyFee(call: unknown): boolean { return evaluatePolicy('financial.fee.charges', { call }) }
export const charges_coldkey_fee = chargesColdkeyFee
export const iter_call_leaves = iterCallLeaves
