import { type ReadView, type ChainRecord, st, num, bal, textRecord, keyOf } from './base'
import { fetchCommitments, decodeCommitmentFields } from '../metagraph'
function ageText(value: number): string {
  let seconds = Math.trunc(value)
  if (seconds < 60) return '<1m'
  const parts: string[] = []
  for (const [suffix, size] of [['d', 86400], ['h', 3600], ['m', 60]] as const) { if (seconds >= size) { parts.push(`${Math.floor(seconds / size)}${suffix}`); seconds %= size }; if (parts.length === 2) break }
  return parts.join(' ')
}
export const identityReads = {
  identity: async (view: ReadView, p: { coldkey_ss58: string }) => textRecord(await view.query(st('IdentitiesV2'), [p.coldkey_ss58])),
  hotkey_identities: async (view: ReadView, p: { hotkey_ss58s: string[] }): Promise<Record<string, ChainRecord>> => {
    const hotkeys = [...new Set(p.hotkey_ss58s)], owners = await view.queryBatch(st('Owner'), hotkeys.map(key => [key]))
    const coldkeys: string[] = [...new Set(owners.filter(Boolean).map(String))]
    const values = await view.queryBatch(st('IdentitiesV2'), coldkeys.map(key => [key]))
    const identities = Object.fromEntries(coldkeys.map((key, i) => [key, textRecord(values[i])]))
    return Object.fromEntries(hotkeys.flatMap((key, i) => owners[i] && identities[String(owners[i])] ? [[key, identities[String(owners[i])]!]] : []))
  },
  commitment: async (view: ReadView, p: { netuid: number; hotkey_ss58: string }) => {
    const value = await view.query(['Commitments', 'CommitmentOf'], [p.netuid, p.hotkey_ss58])
    if (!value) return null
    const fields = value.info?.fields ?? []
    return { block: num(value.block), deposit: bal(view, value.deposit), data: decodeCommitmentFields(fields, false), fields: fields as any[] }
  },
  revealed_commitment: (view: ReadView, p: { netuid: number; hotkey_ss58: string }): Promise<unknown> => view.query(['Commitments', 'RevealedCommitments'], [p.netuid, p.hotkey_ss58]),
  commitments: async (view: ReadView, p: { netuid: number }) => Object.values(await fetchCommitments(view, p.netuid)).map(c => ({ hotkey: c.hotkey, uid: c.uid, commitment: c.value, block: c.block, duration: ageText(c.duration), is_revealed: c.is_revealed, status: c.status, reveals_at: c.reveals_at?.toISOString().replace('.000Z', '+00:00') ?? null })),
}
