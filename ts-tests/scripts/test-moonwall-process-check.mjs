import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { runInNewContext } from "node:vm";

const root = dirname(dirname(fileURLToPath(import.meta.resolve("@moonwall/cli"))));
const files = [
    "dist/cmds/entrypoint.js",
    "dist/cmds/main.js",
    "dist/cmds/runTests.js",
    "dist/cmds/runNetwork.js",
    "dist/internal/fileCheckers.js",
    "dist/internal/launcherCommon.js",
    "dist/internal/index.js",
    "dist/types/src/internal/fileCheckers.js",
];

for (const file of files) {
    // Exercise each published copy of the pinned dependency's actual function,
    // including the bundled CLI used in CI, without starting a network.
    const source = readFileSync(join(root, file), "utf8");
    const body = source.match(/function checkAlreadyRunning\(binaryName\) \{[\s\S]*?\n\}/)?.[0];
    assert.ok(body, `Missing process check in ${file}`);
    const executor = body.match(/const stdout = (execFileSync\d*)\(/)?.[1];
    assert.ok(executor, `Process check must execute pgrep directly in ${file}`);
    const load = (execute) => {
        const globals = { console: { log() {} }, [executor]: execute };
        for (const name of body.match(/\bchalk\d*/g) ?? []) {
            globals[name] = { bgWhiteBright: { blackBright: (value) => value } };
        }
        return runInNewContext(`(${body})`, globals);
    };

    test(`${file}: direct pgrep has scheduling headroom and preserves PIDs`, () => {
        const check = load((command, args, options) => {
            assert.equal(command, "pgrep");
            assert.deepEqual(Array.from(args), ["-x", "node-subtensor"]);
            assert.equal(options.encoding, "utf8");
            assert.equal(options.timeout, 30_000);
            return "12\n34\n";
        });
        assert.deepEqual(Array.from(check("node-subtensor")), [12, 34]);
    });

    for (const [label, status, code] of [
        ["no matching process", 1, undefined],
        ["scan failure", 2, undefined],
        ["timeout", null, "ETIMEDOUT"],
    ]) {
        test(`${file}: ${label}`, () => {
            const error = Object.assign(new Error(label), { status, code });
            const check = load(() => {
                throw error;
            });
            if (status === 1) assert.deepEqual(Array.from(check("node-subtensor")), []);
            else
                assert.throws(
                    () => check("node-subtensor"),
                    (actual) => actual === error
                );
        });
    }
}
