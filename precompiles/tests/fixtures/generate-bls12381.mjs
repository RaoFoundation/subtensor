// Regenerate with tools installed outside the repository:
// npm install --prefix /tmp/bls2537-solidity-tools --no-save --package-lock=false solc@0.8.21 @noble/curves@1.9.7
// node precompiles/tests/fixtures/generate-bls12381.mjs /tmp/bls2537-solidity-tools/node_modules
import { readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { resolve } from "node:path";

const require = createRequire(resolve(process.argv[2], "package.json"));
const solc = require("solc");
const { bls12_381: bls } = require("@noble/curves/bls12-381");
if (!solc.version().startsWith("0.8.21+")) throw new Error("Expected solc 0.8.21");
const nobleVersion = JSON.parse(readFileSync(resolve(process.argv[2], "@noble/curves/package.json"), "utf8")).version;
if (nobleVersion !== "1.9.7") throw new Error("Expected @noble/curves 1.9.7");
const source = readFileSync(new URL("Bls12381Example.sol", import.meta.url), "utf8");
const compiled = JSON.parse(solc.compile(JSON.stringify({
    language: "Solidity",
    sources: { "Bls12381Example.sol": { content: source } },
    settings: {
        optimizer: { enabled: true, runs: 200 },
        evmVersion: "paris",
        outputSelection: { "*": { "*": ["evm.deployedBytecode.object"] } },
    },
})));
for (const diagnostic of compiled.errors ?? []) {
    if (diagnostic.severity === "error") throw new Error(diagnostic.formattedMessage);
}
const field = (n) => n.toString(16).padStart(128, "0");
const encodeG1 = (p) => { const { x, y } = p.toAffine(); return field(x) + field(y); };
const encodeG2 = (p) => { const { x, y } = p.toAffine(); return field(x.c0) + field(x.c1) + field(y.c0) + field(y.c1); };
const DST = "BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_POP_";
const key = 7n;
const publicKey = encodeG1(bls.G1.ProjectivePoint.fromHex(bls.getPublicKey(key)));
const vectors = ["", "Bittensor EIP-2537", "message crossing the SHA-256 block boundary ".repeat(4)].map((text) => {
    const message = new TextEncoder().encode(text);
    const signature = bls.sign(message, key, { DST });
    if (!bls.verify(signature, message, bls.getPublicKey(key), { DST })) throw new Error("Bad vector");
    return { message: Buffer.from(message).toString("hex"), publicKey, signature: encodeG2(bls.G2.ProjectivePoint.fromHex(signature)) };
});
const contracts = compiled.contracts["Bls12381Example.sol"];
writeFileSync(new URL("bls12381-solidity.json", import.meta.url), JSON.stringify({
    compiler: solc.version(), curveLibrary: "@noble/curves@1.9.7", ciphersuite: DST,
    verifier: contracts.Bls12381Example.evm.deployedBytecode.object,
    harness: contracts.Bls12381CallHarness.evm.deployedBytecode.object,
    vectors,
}, null, 2) + "\n");
