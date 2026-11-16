import { evaluatePolicy } from './policy'
import { bytesHex } from './portable'

interface Charge { address: string; amount: bigint }
interface Reserve { address: string; required: bigint; available: bigint }
interface Requirement { address: string; amount: bigint; available: bigint; affordable: boolean }
/** Account decoding and pinned balance reads; arithmetic lives in the core. */
export async function transactionAffordability(input: { spends: Charge[]; charges: Charge[]; reserves: Reserve[]; reserveCharges: Charge[] }, decode: (address: string) => Uint8Array, readFree: (address: string) => Promise<bigint>): Promise<{ required: Requirement[]; reserves: Requirement[] }> {
  const addresses = new Map<string, string>()
  const account = (address: string) => { const key = bytesHex(decode(address)); addresses.set(key, address); return key }
  const charge = (row: Charge) => ({ account: account(row.address), amount: row.amount })
  const facts = { spends: input.spends.map(charge), charges: input.charges.map(charge), reserves: input.reserves.map(row => ({ account: account(row.address), required: row.required, available: row.available })), reserve_charges: input.reserveCharges.map(charge) }
  type Result = { required: Array<{ account: string; amount: string; available: string | null; affordable: boolean | null }>; reserves: Array<{ account: string; required: string; available: string; affordable: boolean }> }
  const requests = evaluatePolicy<Result>('financial.affordability', facts)
  const free = Object.fromEntries(await Promise.all(requests.required.map(async row => [row.account, await readFree(addresses.get(row.account)!)])))
  const result = evaluatePolicy<Result>('financial.affordability', { ...facts, free })
  return { required: result.required.map(row => ({ address: addresses.get(row.account)!, amount: BigInt(row.amount), available: BigInt(row.available!), affordable: row.affordable === true })), reserves: result.reserves.map(row => ({ address: addresses.get(row.account)!, amount: BigInt(row.required), available: BigInt(row.available), affordable: row.affordable })) }
}
