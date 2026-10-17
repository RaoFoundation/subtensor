// Decode finalized localnet blocks with Polkadot.js and unchanged PAPI codecs.
// Usage: node SCRIPT CAPTURE.json RESULTS.json
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { createHash } = require("node:crypto");
const { createRequire } = require("node:module");
const { pathToFileURL } = require("node:url");
const root = path.resolve(__dirname, "../../..");
const req = createRequire(path.join(root, "ts-tests/package.json"));
const { TypeRegistry, Metadata } = req("@polkadot/types");
const { blake2AsHex } = req("@polkadot/util-crypto");
const papiReq = createRequire(req.resolve("polkadot-api"));
const bindings = papiReq("@polkadot-api/substrate-bindings");
const { getDynamicBuilder, getLookupFn } = papiReq("@polkadot-api/metadata-builders");
const capture = fs.readFileSync(process.argv[2]);
const data = JSON.parse(capture);
const bytes = (value) => Uint8Array.from(Buffer.from(value.slice(2), "hex"));
const hex = (value) => `0x${Buffer.from(value).toString("hex")}`;
const sha256 = (value) => createHash("sha256").update(value).digest("hex");
const packageVersion = (entry) =>
    JSON.parse(fs.readFileSync(path.join(path.dirname(entry), "../package.json"))).version;
(async () => {
    const installedCtxPath = new URL(
        "./esm/utils/create-metadata-ctx.mjs",
        pathToFileURL(papiReq.resolve("@polkadot-api/observable-client"))
    );
    const ctxPath = process.env.PAPI_RUNTIME_CONTEXT
        ? pathToFileURL(path.resolve(process.env.PAPI_RUNTIME_CONTEXT))
        : installedCtxPath;
    // An upstream copy can independently verify that the repository's existing
    // submission/block-tracking patches did not alter the decoder under test.
    assert.equal(sha256(fs.readFileSync(ctxPath)), sha256(fs.readFileSync(installedCtxPath)));
    const { createRuntimeCtx } = await import(ctxPath.href);
    const result = {
        capture_sha256: sha256(capture),
        script_sha256: sha256(fs.readFileSync(__filename)),
        lockfile_sha256: sha256(fs.readFileSync(path.join(root, "ts-tests/pnpm-lock.yaml"))),
        papi_runtime_context: {
            source: ctxPath.href,
            sha256: sha256(fs.readFileSync(ctxPath)),
            identical_to_repository_decoder: true,
        },
        papi_packages: Object.fromEntries(
            [
                "@polkadot-api/observable-client",
                "@polkadot-api/substrate-bindings",
                "@polkadot-api/metadata-builders",
            ].map((name) => [name, packageVersion(papiReq.resolve(name))])
        ),
        six_wallet_block: data.six_wallet_block,
        polkadot_js_version: req("@polkadot/types/packageInfo").packageInfo.version,
        limitations: [
            "Codec compatibility, not a graphical Polkadot.js application test",
            "Local functional runtime uses test weights",
            "Repository PAPI retains its existing submission/block-tracking patches; its decoder is unchanged",
        ],
        metadata: {},
    };
    for (const version of [14, 15, 16]) {
        const registry = new TypeRegistry();
        registry.setMetadata(new Metadata(registry, data.metadata[version]));
        const metadata = bindings.unifyMetadata(bindings.metadata.dec(data.metadata[version]));
        const ctx = createRuntimeCtx(metadata, bytes(data.metadata[version]), data.genesis);
        const builder = getDynamicBuilder(getLookupFn(metadata));
        const ext = metadata.extrinsic;
        const params = metadata.lookup[ext.type]?.params;
        const id = (name) => params.find((param) => param.name === name).type;
        const address = builder.buildDefinition(ext.address ?? id("Address"));
        const signature = builder.buildDefinition(ext.signature ?? id("Signature"));
        const call = builder.buildDefinition(ext.call ?? id("Call"));
        const extra = bindings.Tuple(...ext.signedExtensions[0].map((e) => builder.buildDefinition(e.type)));
        const body = bindings.Tuple(address, signature, extra, call);
        const decoder = bindings.createDecoder((input) => {
            const len = bindings.compactNumber.dec(input);
            const format = bindings.extrinsicFormat.dec(input);
            assert.notEqual(format.type, "general");
            return { len, format, value: (format.type === "signed" ? body : call).dec(input) };
        });
        let count = 0;
        const profiles = new Set();
        for (const [blockHash, block] of Object.entries(data.blocks)) {
            const decodedBlock = registry.createType("SignedBlock", block);
            assert.equal(decodedBlock.block.header.hash.toHex(), blockHash);
            assert.equal(registry.createType("SignedBlock", decodedBlock.toU8a()).toHex(), decodedBlock.toHex());
            for (const [index, wire] of block.block.extrinsics.entries()) {
                const tx = decodedBlock.block.extrinsics[index];
                assert.equal(tx.toHex(), wire);
                assert.equal(tx.hash.toHex(), blake2AsHex(wire));
                const decoded = decoder(wire);
                const codec = decoded.format.type === "signed" ? body : call;
                assert.equal(
                    hex(
                        Buffer.concat([
                            bindings.compactNumber.enc(decoded.len),
                            bindings.extrinsicFormat.enc(decoded.format),
                            codec.enc(decoded.value),
                        ])
                    ),
                    wire
                );
                if (tx.isSigned) ctx.getMortalityFromTx(wire);
                const known = data.submitted[tx.hash.toHex()];
                if (known) {
                    assert.equal(tx.isSigned, true);
                    assert.equal(tx.method.meta.name.toString(), "remark");
                    assert.equal(known.block_hash, blockHash);
                    assert.equal(known.index, index);
                    profiles.add(known.crypto_type);
                    if ([7, 8].includes(known.crypto_type)) {
                        assert.equal(tx.signer.type, "Raw");
                        assert.equal(tx.signer.value.length, 5367);
                        assert.equal(
                            registry.createType("AccountId", tx.signer.toString()).toHex(),
                            hex(tx.signer.value.subarray(0, 32))
                        );
                        assert.equal(tx.signature.toHex(), `0x${"4d".repeat(64)}`);
                    }
                }
                count++;
            }
        }
        assert.deepEqual(
            [...profiles].sort((a, b) => a - b),
            [0, 1, 4, 6, 7, 8]
        );
        result.metadata[version] = {
            finalized_blocks: Object.keys(data.blocks).length,
            exact_extrinsic_roundtrips: count,
            wallet_types: [...profiles],
        };
    }
    fs.writeFileSync(process.argv[3], JSON.stringify(result, null, 2) + "\n");
    console.log(JSON.stringify(result, null, 2));
})().catch((error) => {
    console.error(error);
    process.exitCode = 1;
});
