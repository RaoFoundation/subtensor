import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {CODE_ROOTS, EXCLUDED_CODE_DIRS, EXCLUDED_CODE_FILES} from '../src/lib/code-corpus.mjs';

const appDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const tracePath = path.join(appDir, '.next/server/app/code/search.json/route.js.nft.json');
const trace = JSON.parse(fs.readFileSync(tracePath, 'utf8'));
const tracedFiles = new Set(trace.files.map((file) => path.resolve(path.dirname(tracePath), file)));
const repoRoot = path.resolve(appDir, '../../..');
const excludedDirs = new Set(EXCLUDED_CODE_DIRS);
const excludedFiles = new Set(EXCLUDED_CODE_FILES);
const expectedFiles = [];

function walk(dir) {
  for (const entry of fs.readdirSync(dir, {withFileTypes: true})) {
    const filePath = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      if (!excludedDirs.has(entry.name)) walk(filePath);
    } else if (entry.name.endsWith('.rs') && !excludedFiles.has(entry.name)) {
      expectedFiles.push(filePath);
    }
  }
}

for (const root of CODE_ROOTS) walk(path.join(repoRoot, root));

const missingFiles = expectedFiles.filter((file) => !tracedFiles.has(file));
const tracedCorpusFiles = [...tracedFiles].filter(
  (file) =>
    file.endsWith('.rs') &&
    CODE_ROOTS.some((root) => file.startsWith(path.join(repoRoot, root) + path.sep)),
);

assert.deepEqual(
  missingFiles,
  [],
  `search route trace is missing ${missingFiles.length} corpus files`,
);
assert.equal(
  tracedCorpusFiles.length,
  expectedFiles.length,
  'search route trace contains Rust files excluded from the search corpus',
);

console.log(`search route trace contains all ${expectedFiles.length} corpus files`);
