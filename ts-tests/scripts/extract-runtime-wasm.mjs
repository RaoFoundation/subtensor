import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const RUNTIME_CODE_KEY = "0x3a636f6465";

export function decodeRuntimeWasm(chainSpec) {
    const encodedRuntime = chainSpec?.genesis?.raw?.top?.[RUNTIME_CODE_KEY];
    if (typeof encodedRuntime !== "string" || !/^0x(?:[0-9a-fA-F]{2})+$/.test(encodedRuntime)) {
        throw new Error(`Chain spec is missing a valid ${RUNTIME_CODE_KEY} runtime entry`);
    }

    const runtime = Buffer.from(encodedRuntime.slice(2), "hex");
    if (runtime.length === 0) {
        throw new Error("Chain spec runtime entry decoded to an empty file");
    }
    return runtime;
}

export function extractRuntimeWasm(chainSpecPath, outputPath) {
    const chainSpec = JSON.parse(readFileSync(chainSpecPath, "utf8"));
    const runtime = decodeRuntimeWasm(chainSpec);
    writeFileSync(outputPath, runtime);
    return runtime.length;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
    const [, , chainSpecPath, outputPath] = process.argv;
    if (!chainSpecPath || !outputPath) {
        throw new Error("usage: extract-runtime-wasm.mjs CHAIN_SPEC OUTPUT_WASM");
    }
    const byteLength = extractRuntimeWasm(chainSpecPath, outputPath);
    console.log(`Extracted ${byteLength} runtime bytes from ${chainSpecPath}`);
}
