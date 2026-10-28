import { mkdir, rm, symlink, writeFile } from "node:fs/promises";
import { resolve } from "node:path";

// Run only in the disposable harness checkout, after installing dependencies.
const root = resolve(process.argv[2]);
await rm(`${root}/.papi`, { recursive: true, force: true });
await mkdir(`${root}/.papi`, { recursive: true });
await writeFile(
    `${root}/.papi/polkadot-api.json`,
    JSON.stringify({
        version: 0,
        descriptorPath: ".papi/descriptors",
        entries: {},
        // Workers generate from their selected runtime. PAPI must not reinstall
        // the bundled dependencies against a different runner's pnpm store.
        options: { noDescriptorsPackage: true },
    })
);
await mkdir(`${root}/node_modules/@polkadot-api`, { recursive: true });
await rm(`${root}/node_modules/@polkadot-api/descriptors`, {
    recursive: true,
    force: true,
});
await symlink("../../.papi/descriptors", `${root}/node_modules/@polkadot-api/descriptors`, "dir");
