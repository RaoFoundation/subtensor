import fs from 'node:fs';
import path from 'node:path';
import { execSync } from 'node:child_process';
import { CODE_ROOTS, EXCLUDED_CODE_DIRS, EXCLUDED_CODE_FILES } from './code-corpus.mjs';

export { CODE_ROOTS } from './code-corpus.mjs';

// The on-chain source viewer (/code) reads the Rust straight out of the
// repository checkout at build time, the same way the docs content is read
// from ../../../docs (see source.config.ts). Every page is prerendered, so
// nothing here runs at request time.
const repoRoot = path.resolve(process.cwd(), '../../..');

// What actually runs on (or compiles into) the chain: the pallets, the
// runtime that assembles them, and the shared primitives. Excluded on
// purpose: node/ (the client binary), support/ (proc-macro tooling),
// vendor/ (upstream forks), and all tests, mocks, and benchmark code.
const EXCLUDED_DIRS = new Set(EXCLUDED_CODE_DIRS);
const EXCLUDED_FILES = new Set(EXCLUDED_CODE_FILES);

export interface CodeFile {
  /** Repo-relative path, e.g. "pallets/subtensor/src/coinbase/run_coinbase.rs". */
  path: string;
  lines: number;
  bytes: number;
}

function walk(dir: string, out: CodeFile[]) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const abs = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      if (!EXCLUDED_DIRS.has(entry.name)) walk(abs, out);
    } else if (entry.name.endsWith('.rs') && !EXCLUDED_FILES.has(entry.name)) {
      const text = fs.readFileSync(abs, 'utf8');
      out.push({
        path: path.relative(repoRoot, abs),
        lines: text.split('\n').length - (text.endsWith('\n') ? 1 : 0),
        bytes: Buffer.byteLength(text),
      });
    }
  }
}

let cachedFiles: CodeFile[] | null = null;

/** Every file in the corpus, sorted by path. */
export function codeFiles(): CodeFile[] {
  if (!cachedFiles) {
    const out: CodeFile[] = [];
    for (const root of CODE_ROOTS) walk(path.join(repoRoot, root), out);
    out.sort((a, b) => (a.path < b.path ? -1 : 1));
    cachedFiles = out;
  }
  return cachedFiles;
}

export function getCodeFile(filePath: string): CodeFile | undefined {
  return codeFiles().find((f) => f.path === filePath);
}

export function readCodeFile(filePath: string): string {
  return fs.readFileSync(path.join(repoRoot, filePath), 'utf8');
}

export interface CodeSearchHit {
  path: string;
  line?: number;
  text?: string;
  kind: 'path' | 'content';
  raw_url: string;
  url: string;
}

let cachedContents: Map<string, string> | null = null;

function codeContents(): Map<string, string> {
  if (!cachedContents) {
    const map = new Map<string, string>();
    for (const file of codeFiles()) {
      map.set(file.path, readCodeFile(file.path));
    }
    cachedContents = map;
  }
  return cachedContents;
}

/**
 * Literal substring search over the on-chain Rust corpus (paths + contents).
 * Built for agents: one HTTP hop instead of cloning or fetching every file.
 */
export function searchCode(query: string, limit = 20): CodeSearchHit[] {
  const q = query.trim();
  if (q.length < 2) return [];
  const capped = Math.min(Math.max(limit, 1), 50);
  const hits: CodeSearchHit[] = [];
  const contents = codeContents();

  for (const file of codeFiles()) {
    if (file.path.includes(q)) {
      hits.push({
        path: file.path,
        kind: 'path',
        raw_url: `/code/raw/${file.path}`,
        url: `/code/${file.path}`,
      });
      if (hits.length >= capped) return hits;
    }
  }

  for (const file of codeFiles()) {
    const text = contents.get(file.path);
    if (!text) continue;
    const lines = text.split('\n');
    for (let i = 0; i < lines.length; i++) {
      if (!lines[i].includes(q)) continue;
      const line = i + 1;
      hits.push({
        path: file.path,
        line,
        text: lines[i].trim().slice(0, 200),
        kind: 'content',
        raw_url: `/code/raw/${file.path}`,
        url: `/code/${file.path}#L${line}`,
      });
      if (hits.length >= capped) return hits;
    }
  }

  return hits;
}

export function isCodeDir(dirPath: string): boolean {
  if (dirPath === '') return true;
  const prefix = dirPath + '/';
  return codeFiles().some((f) => f.path.startsWith(prefix));
}

export interface CodeDirListing {
  dirs: { name: string; files: number; lines: number }[];
  files: CodeFile[];
}

/** Immediate children of a corpus directory ('' = the corpus root). */
export function listCodeDir(dirPath: string): CodeDirListing {
  const prefix = dirPath === '' ? '' : dirPath + '/';
  const dirs = new Map<string, { files: number; lines: number }>();
  const files: CodeFile[] = [];
  for (const file of codeFiles()) {
    if (!file.path.startsWith(prefix)) continue;
    const rest = file.path.slice(prefix.length);
    const slash = rest.indexOf('/');
    if (slash === -1) {
      files.push(file);
    } else {
      const name = rest.slice(0, slash);
      const dir = dirs.get(name) ?? { files: 0, lines: 0 };
      dir.files += 1;
      dir.lines += file.lines;
      dirs.set(name, dir);
    }
  }
  return {
    dirs: [...dirs.entries()]
      .map(([name, stats]) => ({ name, ...stats }))
      .sort((a, b) => (a.name < b.name ? -1 : 1)),
    files,
  };
}

/** Every directory path in the corpus (excluding the root itself). */
export function codeDirs(): string[] {
  const dirs = new Set<string>();
  for (const file of codeFiles()) {
    const parts = file.path.split('/');
    for (let i = 1; i < parts.length; i++) {
      dirs.add(parts.slice(0, i).join('/'));
    }
  }
  return [...dirs].sort();
}

let cachedCommit: string | null | undefined;

/** The commit the site was built from — the version stamp on every code page. */
export function buildCommit(): string | null {
  if (cachedCommit === undefined) {
    cachedCommit =
      process.env.VERCEL_GIT_COMMIT_SHA ??
      (() => {
        try {
          return execSync('git rev-parse HEAD', { cwd: repoRoot }).toString().trim();
        } catch {
          return null;
        }
      })();
  }
  return cachedCommit;
}
