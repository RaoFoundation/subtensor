import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { pathToFileURL } from "node:url";
import { setImmediate } from "node:timers/promises";
import { test } from "node:test";

// Exercise the actual pinned dependency, without a node or timing-dependent RPC.
const require = createRequire(import.meta.url);
const papiRequire = createRequire(require.resolve("polkadot-api"));
const clientPath = pathToFileURL(papiRequire.resolve("@polkadot-api/observable-client"));
const clients = {
    cjs: papiRequire("@polkadot-api/observable-client"),
    esm: await import(new URL("./esm/index.mjs", clientPath).href),
};
const { blockHeader, Blake2256 } = papiRequire("@polkadot-api/substrate-bindings");
const { NEVER, BehaviorSubject, Subject, of } = papiRequire("rxjs");
const hex = (value) => `0x${Buffer.from(value).toString("hex")}`;
const zero = `0x${"00".repeat(32)}`;
const header = hex(blockHeader.enc({ parentHash: zero, number: 1, stateRoot: zero, extrinsicRoot: zero, digests: [] }));
const first = hex(Blake2256(Buffer.from(header.slice(2), "hex")));
const second = `0x${"22".repeat(32)}`;
const third = `0x${"33".repeat(32)}`;

for (const [format, { getObservableClient }] of Object.entries(clients)) {
    for (const bestState of ["missing", "behind", "ahead"]) {
        test(`${format}: finalization with best block ${bestState}`, async () => {
            let emit;
            const client = getObservableClient(
                {
                    archive: {},
                    chainHead: (_withRuntime, onEvent) => {
                        emit = onEvent;
                        return {
                            header: async () => header,
                            storage: async () => zero,
                            unpin: async () => {},
                            unfollow: () => {},
                        };
                    },
                    destroy: () => {},
                },
                { getMetadata: () => NEVER }
            );
            const chain = client.chainHead$();
            const errors = [];
            const finalized = [];
            const best = [];
            const subscription = chain.finalized$.subscribe({
                next: (block) => finalized.push(block),
                error: (error) => errors.push(error),
            });
            const bestSubscription = chain.best$.subscribe({
                next: (block) => best.push(block),
                error: (error) => errors.push(error),
            });
            try {
                emit({ type: "initialized", finalizedBlockHashes: [first] });
                await setImmediate();
                if (bestState !== "missing") emit({ type: "bestBlockChanged", bestBlockHash: first });
                emit({ type: "newBlock", blockHash: second, parentBlockHash: first, newRuntime: null });
                if (bestState === "ahead") {
                    emit({ type: "newBlock", blockHash: third, parentBlockHash: second, newRuntime: null });
                    emit({ type: "bestBlockChanged", bestBlockHash: third });
                }
                emit({ type: "finalized", finalizedBlockHashes: [second], prunedBlockHashes: [] });
                if (bestState === "missing") emit({ type: "bestBlockChanged", bestBlockHash: second });
                await setImmediate();
                assert.deepEqual(errors, []);
                assert.equal(finalized.at(-1)?.hash, second);
                assert.equal(finalized.at(-1)?.number, 2);
                assert.equal(best.at(-1)?.hash, bestState === "ahead" ? third : second);
            } finally {
                subscription.unsubscribe();
                bestSubscription.unsubscribe();
                chain.unfollow();
                client.destroy();
            }
        });
    }
}

// Use the installed submission implementation, with deterministic block/analysis streams.
const { submit$ } = await import(
    new URL("./esm/tx/submit-fns.mjs", pathToFileURL(require.resolve("polkadot-api"))).href
);
for (const scenario of ["connected", "unpinned ancestor", "pruned fork", "unknown block", "unfinalized block"]) {
    test(`transaction analysis after ${scenario}`, () => {
        const block = (hash, number, parent) => ({ hash, number, parent, children: new Set(), pruned: false });
        const a = block("a", 1, "genesis");
        const b = block("b", scenario === "unfinalized block" ? 6 : 2, "a");
        const blocks = new Map([["a", a]]);
        const state = {
            best: "a",
            finalized: "a",
            blocks,
            finalizedRuntime: { runtime: of({ getMortalityFromTx: () => ({ mortal: false }) }) },
        };
        const pinned = new BehaviorSubject(state);
        pinned.state = state;
        const tracked = new Subject();
        const events = [];
        const errors = [];
        const subscription = submit$(
            {
                hasher$: of(() => new Uint8Array(32)),
                pinnedBlocks$: pinned,
                validateTx$: () => of({ success: true }),
                finalized$: of(a),
                trackTx$: () => tracked,
            },
            () => NEVER,
            "0x00"
        ).subscribe({ next: (event) => events.push(event), error: (error) => errors.push(error) });
        try {
            blocks.delete("a");
            if (scenario !== "unknown block") blocks.set("b", b);
            if (scenario === "connected") blocks.set("c", block("c", 3, "b"));
            if (scenario === "pruned fork") b.pruned = true;
            blocks.set("d", block("d", 4, "c"));
            blocks.set("e", block("e", 5, "d"));
            Object.assign(state, { best: "e", finalized: "d" });
            pinned.next({ ...state });
            tracked.next({
                hash: "b",
                found: {
                    type: true,
                    index: 0,
                    events: [
                        {
                            phase: { type: "ApplyExtrinsic", value: 0 },
                            event: { type: "System", value: { type: "ExtrinsicSuccess" } },
                            topics: [],
                        },
                    ],
                },
            });
            assert.deepEqual(errors, []);
            const finalized = events.filter((event) => event.type === "finalized");
            const shouldFinalize = scenario === "connected" || scenario === "unpinned ancestor";
            assert.equal(finalized.length, shouldFinalize ? 1 : 0);
            if (shouldFinalize) {
                assert.equal(finalized[0].block.hash, "b");
                assert.equal(finalized[0].block.number, 2);
                assert.equal(finalized[0].ok, true);
            }
        } finally {
            subscription.unsubscribe();
        }
    });
}
