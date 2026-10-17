// Usage: node ml-dsa-papi-codec.cjs FIXTURE OUTPUT [SDK_CALL_RESULTS]
// Uses the repository's installed pinned PAPI packages without modifying them.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { createHash } = require("node:crypto");
const { createRequire } = require("node:module");
const { pathToFileURL } = require("node:url");

const root = path.resolve(__dirname, "../../..");
const req = createRequire(path.join(root, "ts-tests/package.json"));
const papiEntry = req.resolve("polkadot-api");
const papiReq = createRequire(papiEntry);
const bindings = papiReq("@polkadot-api/substrate-bindings");
const { getDynamicBuilder, getLookupFn } = papiReq("@polkadot-api/metadata-builders");
const [fixturePath, outputPath, operationsPath] = process.argv.slice(2);
const operations = operationsPath ? JSON.parse(fs.readFileSync(operationsPath)).extrinsics : [];
if (!fixturePath) throw new Error("Usage: node ml-dsa-papi-codec.cjs FIXTURE [OUTPUT]");
const fixture = fs.readFileSync(fixturePath);
const data = JSON.parse(fixture);
const sha256 = (value) => createHash("sha256").update(value).digest("hex");
const bytes = (value) => Uint8Array.from(Buffer.from(value.slice(2), "hex"));
const hex = (value) => `0x${Buffer.from(value).toString("hex")}`;
const packageVersion = (entry) =>
    JSON.parse(fs.readFileSync(path.join(path.dirname(entry), "../package.json"), "utf8")).version;

(async () => {
    assert.equal(data.extrinsics.length, 7);
    const observableEntry = papiReq.resolve("@polkadot-api/observable-client");
    const ctxPath = new URL("./esm/utils/create-metadata-ctx.mjs", pathToFileURL(observableEntry));
    const { createRuntimeCtx } = await import(ctxPath.href);
    const result = {
        fixture_sha256: sha256(fixture),
        sdk_call_evidence_sha256: operationsPath ? sha256(fs.readFileSync(operationsPath)) : null,
        script_sha256: sha256(fs.readFileSync(__filename)),
        lockfile_sha256: sha256(fs.readFileSync(path.join(root, "ts-tests/pnpm-lock.yaml"))),
        packages: {
            "polkadot-api": packageVersion(papiEntry),
            "@polkadot-api/observable-client": packageVersion(observableEntry),
            "@polkadot-api/substrate-bindings": packageVersion(papiReq.resolve("@polkadot-api/substrate-bindings")),
            "@polkadot-api/metadata-builders": packageVersion(papiReq.resolve("@polkadot-api/metadata-builders")),
        },
        runtime_context_sha256: sha256(fs.readFileSync(ctxPath)),
        metadata: {},
    };
    for (const version of [14, 15, 16]) {
        const metadata = bindings.unifyMetadata(bindings.metadata.dec(data.metadata[version]));
        const ctx = createRuntimeCtx(metadata, bytes(data.metadata[version]), `0x${"00".repeat(32)}`);
        const builder = getDynamicBuilder(getLookupFn(metadata));
        const ext = metadata.extrinsic;
        const params = metadata.lookup[ext.type]?.params;
        const id = (name) => params.find((param) => param.name === name).type;
        const address = builder.buildDefinition(ext.address ?? id("Address"));
        const signature = builder.buildDefinition(ext.signature ?? id("Signature"));
        const call = builder.buildDefinition(ext.call ?? id("Call"));
        const extra = bindings.Tuple(
            ...ext.signedExtensions[0].map((extension) => builder.buildDefinition(extension.type))
        );
        const body = bindings.Tuple(address, signature, extra, call);
        // The actual PAPI runtime-context decoder is exercised separately below.
        // This public-codec composition additionally checks the complete call
        // and exact byte roundtrip, which its mortality-only API does not expose.
        const decoder = bindings.createDecoder((input) => {
            const len = bindings.compactNumber.dec(input);
            const format = bindings.extrinsicFormat.dec(input);
            assert.notEqual(format.type, "general");
            return { len, format, value: (format.type === "signed" ? body : call).dec(input) };
        });
        for (const [index, tx] of data.extrinsics.entries()) {
            assert.deepEqual(ctx.getMortalityFromTx(tx), { mortal: false });
            const decoded = decoder(tx);
            const codec = decoded.format.type === "signed" ? body : call;
            const roundtrip = Buffer.concat([
                bindings.compactNumber.enc(decoded.len),
                bindings.extrinsicFormat.enc(decoded.format),
                codec.enc(decoded.value),
            ]);
            assert.equal(hex(roundtrip), tx);
            const decodedCall = decoded.format.type === "signed" ? decoded.value[3] : decoded.value;
            assert.equal(decodedCall.type, "System");
            assert.equal(decodedCall.value.type, "remark");
            if (index === 1) {
                assert.equal(decoded.value[0].type, "Raw");
                assert.equal(decoded.value[0].value.asBytes().length, 5367);
                assert.equal(decoded.value[1].type, "Ed25519");
                assert.equal(decoded.value[1].value.asHex(), `0x${"4d".repeat(64)}`);
            }
        }
        for (const operation of operations) {
            assert.deepEqual(ctx.getMortalityFromTx(operation.hex), { mortal: false });
            const decoded = decoder(operation.hex);
            assert.equal(decoded.format.type, "signed");
            assert.equal(hex(Buffer.concat([
                bindings.compactNumber.enc(decoded.len),
                bindings.extrinsicFormat.enc(decoded.format),
                body.enc(decoded.value),
            ])), operation.hex);
            assert.equal(decoded.value[3].type, operation.module);
            assert.equal(decoded.value[3].value.type, operation.function);
        }
        result.metadata[version] = {
            consolidated_calls_decoded: operations.length,
            actual_papi_mortality_decoder: 7,
            dynamic_complete_body_roundtrips: 7,
            mldsa_raw_length: 5367,
            call: "System.remark",
        };
    }
    const output = `${JSON.stringify(result, null, 2)}\n`;
    if (outputPath) fs.writeFileSync(outputPath, output);
    process.stdout.write(output);
})().catch((error) => {
    console.error(error);
    process.exitCode = 1;
});
