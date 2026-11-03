import { READ_CATALOG, type ReadName, type ReadParams } from './read-catalog'
import { type ReadView } from './reads/base'
import { accountReads } from './reads/accounts'
import { subnetReads } from './reads/subnets'
import { stakingReads } from './reads/staking'
import { epochReads } from './reads/epochs'
import { delegationReads } from './reads/delegation'
import { leasingReads } from './reads/leasing'
import { collateralReads } from './reads/collateral'
import { rootReads } from './reads/root'
import { lockReads } from './reads/locks'
import { identityReads } from './reads/identity'
import { weightReads } from './reads/weights'

export { READ_CATALOG }
export type { ReadName, ReadParams, ReadView }
export type { ReadAddress } from './read-catalog'
export type { ReadStakePosition, StakeValuation } from './reads/staking'
export type { SubnetLifecycleState } from './reads/subnets'
export const READ_HANDLERS = { ...accountReads, ...subnetReads, ...stakingReads, ...epochReads, ...delegationReads, ...leasingReads, ...collateralReads, ...rootReads, ...lockReads, ...identityReads, ...weightReads }
export type ReadArgs<Name extends ReadName> = {} extends ReadParams[Name] ? [params?: ReadParams[Name]] : [params: ReadParams[Name]]
export type ReadResult<Name extends ReadName> = Awaited<ReturnType<(typeof READ_HANDLERS)[Name]>>
// Compile-time exhaustiveness: adding a Python read to the catalog requires an implementation.
const exhaustive: Record<ReadName, (view: ReadView, params: any) => Promise<any>> = READ_HANDLERS
void exhaustive

export function listReads() { return READ_CATALOG.map(row => ({ ...row, params: { ...row.params }, defaults: { ...row.defaults } })) }
export async function dispatchRead<Name extends ReadName>(view: ReadView, name: Name, ...args: ReadArgs<Name>): Promise<ReadResult<Name>>
export async function dispatchRead(view: ReadView, name: string, params?: Record<string, unknown>): Promise<unknown>
export async function dispatchRead(view: ReadView, name: string, params: Record<string, unknown> = {}): Promise<unknown> {
  const spec = READ_CATALOG.find(row => row.name === name)
  if (!spec) throw new Error(`unknown read ${name}`)
  for (const key of Object.keys(params)) if (!(key in spec.params)) throw new TypeError(`unknown parameter ${key} for ${name}`)
  const normalized: Record<string, unknown> = { ...spec.defaults, ...params }
  for (const [key, kind] of Object.entries(spec.params)) {
    let value = normalized[key]
    if (key.endsWith('_ss58s') && Array.isArray(value)) value = normalized[key] = await Promise.all(value.map(entry => readAddress(entry, key)))
    else if (key.endsWith('_ss58') && value != null) value = normalized[key] = await readAddress(value, key)
    if (value == null) {
      if (!(key in spec.defaults)) throw new TypeError(`missing parameter ${key} for ${name}`)
      continue
    }
    if (kind === 'integer' && (!Number.isSafeInteger(value) || Number(value) < 0)) throw new TypeError(`${key} must be a non-negative safe integer`)
    if (kind === 'boolean' && typeof value !== 'boolean') throw new TypeError(`${key} must be boolean`)
    if (kind === 'string' && typeof value !== 'string') throw new TypeError(`${key} must be a string`)
    if (kind === 'array' && !Array.isArray(value)) throw new TypeError(`${key} must be an array`)
    if (kind === 'number' && typeof value !== 'number' && typeof value !== 'string') throw new TypeError(`${key} must be an amount`)
  }
  // A read and all its dependent requests use the same block, including fallback paths.
  return READ_HANDLERS[name as ReadName](await view.at(), normalized as never)
}

async function readAddress(value: unknown, param: string, seen = new Set<object>()): Promise<string> {
  if (typeof value === 'string') return value
  if (value == null || typeof value !== 'object') throw new TypeError(`${param} must be an address or public key-bearing object`)
  if (seen.has(value)) throw new TypeError(`${param} contains a cyclic public wallet key`)
  seen.add(value)
  const record = value as Record<string, unknown>
  const address = record.ss58Address ?? record.ss58_address ?? record.address
  if (typeof address === 'string') return address
  const publicKey = record[param.includes('hotkey') ? 'hotkeypub' : 'coldkeypub']
  if (publicKey != null && publicKey !== value) return readAddress(await publicKey, param, seen)
  throw new TypeError(`${param} must expose an address or a public wallet key`)
}
