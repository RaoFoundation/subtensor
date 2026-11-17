import { JsonRpcTransport, type JsonRpcTransportOptions } from './transport'

export interface EndpointProbe {
  readonly endpoint: string
  readonly ok: boolean
  /** Connection establishment plus one header query, in milliseconds. */
  readonly latencyMs: number
  readonly blockNumber?: number
  readonly error?: string
}

/** Measure explicit endpoints without submitting transactions or changing client configuration. */
export async function probeEndpoints(endpoints: readonly string[], options: JsonRpcTransportOptions & { signal?: AbortSignal; concurrency?: number } = {}): Promise<EndpointProbe[]> {
  const concurrency = options.concurrency ?? 4
  if (!Number.isSafeInteger(concurrency) || concurrency < 1 || concurrency > 32) throw new RangeError('concurrency must be between 1 and 32')
  const targets = [...new Set(endpoints)]
  for (const endpoint of targets) if (!['http:', 'https:', 'ws:', 'wss:'].includes(new URL(endpoint).protocol)) throw new TypeError('endpoint must use HTTP(S) or WS(S)')
  const results: EndpointProbe[] = new Array(targets.length)
  let next = 0
  await Promise.all(Array.from({ length: Math.min(concurrency, targets.length) }, async () => {
    for (;;) {
      options.signal?.throwIfAborted()
      const index = next++
      if (index >= targets.length) return
      const endpoint = targets[index], start = performance.now()
      const transport = new JsonRpcTransport(endpoint, [], false, { ...options, requestTimeoutMs: options.requestTimeoutMs ?? 10_000, maxRequestRetries: 0 })
      try {
        const header = await transport.request('chain_getHeader', [], { signal: options.signal }) as { number?: string }
        const rawNumber: unknown = header?.number
        const blockNumber = Number(rawNumber)
        if (!(typeof rawNumber === 'number' || typeof rawNumber === 'string' && /^(?:\d+|0x[0-9a-f]+)$/i.test(rawNumber)) || !Number.isSafeInteger(blockNumber) || blockNumber < 0) throw new Error('endpoint returned an invalid header')
        results[index] = { endpoint, ok: true, latencyMs: performance.now() - start, blockNumber }
      } catch (error) {
        options.signal?.throwIfAborted()
        results[index] = { endpoint, ok: false, latencyMs: performance.now() - start, error: error instanceof Error ? error.message : String(error) }
      } finally { transport.close() }
    }
  }))
  return results.sort((a, b) => Number(b.ok) - Number(a.ok) || a.latencyMs - b.latencyMs)
}
