// npm install --prefix /tmp/hotkey-growth-solc --no-save --package-lock=false solc@0.8.21
// node precompiles/tests/fixtures/generate-staking-growth.mjs
import { createRequire } from "node:module";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const solc = require("/tmp/hotkey-growth-solc/node_modules/solc");
if (!solc.version().startsWith("0.8.21+")) throw new Error("Expected solc 0.8.21");
const dir = path.dirname(fileURLToPath(import.meta.url));
const source = "staking-growth.sol";
const result = JSON.parse(solc.compile(JSON.stringify({
    language: "Solidity",
    sources: { [source]: { content: fs.readFileSync(path.join(dir, source), "utf8") } },
    settings: { evmVersion: "paris", optimizer: { enabled: true, runs: 200 }, outputSelection: { "*": { "*": ["evm.deployedBytecode.object"] } } },
})));
if (result.errors?.some((item) => item.severity === "error")) throw new Error(JSON.stringify(result.errors));
fs.writeFileSync(path.join(dir, "staking-growth.hex"), result.contracts[source].StakingGrowthFixture.evm.deployedBytecode.object + "\n");
