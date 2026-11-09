#!/usr/bin/env node
// Read-only mainnet calibration. Uses ts-tests' installed @polkadot/api.
const { createRequire } = require('node:module');
const { resolve } = require('node:path');
const { readFileSync, writeFileSync } = require('node:fs');
const load = createRequire(resolve(__dirname, '../ts-tests/package.json'));
const { ApiPromise, WsProvider, HttpProvider } = load('@polkadot/api');

function writeOutputs(result) {
    if (process.env.BASKET_RESEARCH_OUTPUT) writeFileSync(process.env.BASKET_RESEARCH_OUTPUT, JSON.stringify(result, null, 2) + '\n');
    if (!process.env.BASKET_RESEARCH_COMPACT_OUTPUT) return;
    const target = '5GKH9FPPnWSUoeeTJp19wVtd84XqFW4pyK2ijV2GsFbhTrP1';
    writeFileSync(process.env.BASKET_RESEARCH_COMPACT_OUTPUT, JSON.stringify({
        ...result, baskets: undefined, samples: result.samples.map(s => ({
            block: s.block, hash: s.hash, funds: s.funds.length,
            eligible: s.funds.filter(f => !f.missing).length,
            oldGatePass: s.funds.filter(f => !f.missing && f.outside === 0).length,
            netGatePass: [5, 10, 15, 25, 50].map(bps => ({ bps, count: s.funds.filter(f => !f.missing && Math.abs(f.netBps) <= bps).length })),
            taostats: s.funds.find(f => f.hotkey === target),
        })),
    }, null, 2) + '\n');
}

async function main() {
    const endpoint = process.env.BASKET_RESEARCH_RPC || 'wss://archive.chain.opentensor.ai:443';
    const provider = endpoint.startsWith('http') ? new HttpProvider(endpoint) : new WsProvider(endpoint);
    const api = await ApiPromise.create({
        provider,
        noInitWarn: true,
    });
    try {
        api.registerTypes({
            ResearchHolding: { netuid: 'u16', alpha: 'u64', spot_tao: 'u64', realizable_tao: 'u64' },
            ResearchBasket: { hotkey: 'AccountId', nav_tao: 'u64', spot_nav_tao: 'u64', shares: 'u64', deposited_tao: 'u64', redeemed_tao: 'u64', holdings: 'Vec<ResearchHolding>' },
        });
        const endHash = process.env.BASKET_RESEARCH_POINT || process.env.BASKET_RESEARCH_END
            ? await api.rpc.chain.getBlockHash(Number(process.env.BASKET_RESEARCH_POINT || process.env.BASKET_RESEARCH_END))
            : await api.rpc.chain.getFinalizedHead();
        const end = (await api.rpc.chain.getHeader(endHash)).number.toNumber();
        const start = Number(process.env.BASKET_RESEARCH_POINT || process.env.BASKET_RESEARCH_START || end - 30 * 7200);
        const step = Number(process.env.BASKET_RESEARCH_STEP || 720);
        const raw = await provider.send('state_call', ['BetaBasketRuntimeApi_get_all_validator_baskets', '0x', endHash.toHex()]);
        const baskets = api.createType('Vec<ResearchBasket>', raw).toJSON()
            .filter(b => !process.env.BASKET_RESEARCH_HOTKEY || b.hotkey === process.env.BASKET_RESEARCH_HOTKEY);
        const pending = new Map();
        if (process.env.BASKET_RESEARCH_POINT) {
            for (const b of baskets.filter(b => b.holdings.length >= 100)) {
                const prefix = api.query.subtensorModule.pendingBasketDeposits.keyPrefix(b.hotkey);
                const keys = await provider.send('state_getKeysPaged', [prefix, 1000, null, endHash.toHex()]);
                if (keys.length === 1000) throw new Error('Pending prefix pagination required');
                const changes = keys.length ? await provider.send('state_queryStorageAt', [keys, endHash.toHex()]) : [];
                pending.set(b.hotkey, changes.flatMap(c => c.changes).filter(([, v]) => v !== null).map(([key, value]) => {
                    const bytes = Buffer.from(key.slice(2), 'hex');
                    return { netuid: bytes[bytes.length - 2] + 256 * bytes[bytes.length - 1], alpha: Number(api.createType('u64', Buffer.from(value.slice(2), 'hex')).toBigInt()) };
                }));
            }
        }
        const netuids = [...new Set(baskets.flatMap(b => b.holdings.map(h => h.netuid)))].sort((a, b) => a - b);
        const fastKeys = netuids.map(n => api.query.subtensorModule.subnetFastMovingPrice.key(n));
        const thresholds = [5, 10, 15, 25, 50];
        const samples = [];
        for (let block = start; block <= end; block += step) {
            const hash = await api.rpc.chain.getBlockHash(block);
            const [priceRaw, pairs] = await Promise.all([
                provider.send('state_call', ['SwapRuntimeApi_current_alpha_price_all', '0x', hash.toHex()]),
                provider.send('state_queryStorageAt', [fastKeys, hash.toHex()]),
            ]);
            const prices = new Map(api.createType('Vec<(u16,u64)>', priceRaw).map(x => [x[0].toNumber(), Number(x[1].toString()) / 1e9]));
            const fast = new Map(pairs.flatMap(changes => changes.changes).filter(([, value]) => value !== null).map(([key, value]) => {
                const bytes = Buffer.from(key.slice(2), 'hex');
                return [bytes[bytes.length - 2] + 256 * bytes[bytes.length - 1], Number(api.createType('u128', Buffer.from(value.slice(2), 'hex')).toBigInt()) / 2 ** 64];
            }));
            const funds = baskets.filter(b => b.holdings.length >= 100).map(b => {
                let live = 0, anchor = 0, gross = 0, outside = 0, missing = 0;
                let realizableLive = 0, realizableReference = 0;
                for (const h of b.holdings) {
                    const spot = h.netuid === 0 ? 1 : prices.get(h.netuid);
                    const ema = h.netuid === 0 ? 1 : fast.get(h.netuid);
                    if (!spot || !ema) { missing++; continue; }
                    const value = Number(h.alpha) * spot;
                    const unstable = Math.abs(spot / ema - 1) > 0.02;
                    const reference = unstable ? Number(h.alpha) * ema : value;
                    live += value; anchor += reference;
                    realizableLive += Number(h.realizable_tao);
                    realizableReference += Number(h.realizable_tao) * reference / value;
                    gross += Math.abs(value - reference);
                    if (unstable) outside++;
                }
                let added = 0, referenceAdded = 0, originError = 0;
                for (const credit of pending.get(b.hotkey) || []) {
                    const spot = prices.get(credit.netuid), ema = fast.get(credit.netuid);
                    if (!spot || !ema || credit.alpha * ema < 500000) continue;
                    const value = credit.alpha * spot;
                    const reference = credit.alpha * ema;
                    added += value; referenceAdded += reference; originError += Math.abs(value - reference);
                }
                return { hotkey: b.hotkey, holdings: b.holdings.length, outside, missing, netBps: anchor ? (live / anchor - 1) * 10000 : null, grossBps: anchor ? gross / anchor * 10000 : null,
                    ...(pending.has(b.hotkey) ? { realizableNavTao: realizableLive / 1e9, realizableNetBps: realizableReference ? (realizableLive / realizableReference - 1) * 10000 : null,
                        pendingRows: pending.get(b.hotkey).length, addedTao: added / 1e9, originGrossBps: referenceAdded ? originError / referenceAdded * 10000 : 0, mintBps: referenceAdded && live ? (added * anchor / (referenceAdded * live) - 1) * 10000 : 0 } : {}) };
            });
            samples.push({ block, hash: hash.toHex(), funds });
            if (process.env.BASKET_RESEARCH_OUTPUT) writeFileSync(process.env.BASKET_RESEARCH_OUTPUT, JSON.stringify({ finalizedBlock: end, start, step, baskets, samples }, null, 2) + '\n');
            if (samples.length % 20 === 0) process.stderr.write(`sampled ${samples.length} blocks (through ${block})\n`);
            await new Promise(done => setTimeout(done, 1500));
        }
        const usable = samples.flatMap(s => s.funds).filter(f => !f.missing && f.netBps !== null);
        const summary = { observations: usable.length, oldGatePassPct: 100 * usable.filter(f => f.outside === 0).length / usable.length,
            thresholds: thresholds.map(bps => ({ bps, netPassPct: 100 * usable.filter(f => Math.abs(f.netBps) <= bps).length / usable.length, grossPassPct: 100 * usable.filter(f => f.grossBps <= bps).length / usable.length })) };
        const result = { chain: (await api.rpc.system.chain()).toString(), endpoint, finalizedBlock: end, finalizedHash: endHash.toHex(), start, step, methodology: 'Frozen end-block basket alpha quantities (large baskets >=100 holdings), historical spot prices and fast EMA; out-of-band holding contributions only, all incoming origins anchored. Spot valuation proxy, not a historical replay of realizable share mints. Missing anchors excluded, never counted as passing.', summary, baskets, pending: Object.fromEntries(pending), samples };
        writeOutputs(result);
        console.log(JSON.stringify({ finalizedBlock: end, start, step, samples: samples.length, summary }, null, 2));
    } finally { await api.disconnect(); }
}
if (process.env.BASKET_RESEARCH_INPUT) {
    writeOutputs(JSON.parse(readFileSync(process.env.BASKET_RESEARCH_INPUT, 'utf8')));
} else {
    main().catch(error => { console.error(error); process.exitCode = 1; });
}
