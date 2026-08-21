import {createMDX} from 'fumadocs-mdx/next';
import {mkdirSync, readdirSync, readFileSync, writeFileSync} from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';

const withMDX = createMDX();

// Keep in sync with CODE_ROOTS / exclusions in src/lib/code.ts.
const CODE_ROOTS = [
  'pallets',
  'runtime',
  'primitives',
  'common',
  'precompiles',
  'chain-extensions',
];
const EXCLUDED_DIRS = new Set(['tests', 'target']);
const EXCLUDED_FILES = new Set(['tests.rs', 'mock.rs', 'benchmarks.rs', 'benchmarking.rs']);

const appDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(appDir, '../../..');

/** Bundle the /code corpus so search.json does not walk the repo at request time. */
function writeCodeCorpus() {
  /** @type {Record<string, string>} */
  const corpus = {};
  function walk(dir) {
    for (const entry of readdirSync(dir, {withFileTypes: true})) {
      const abs = path.join(dir, entry.name);
      if (entry.isDirectory()) {
        if (!EXCLUDED_DIRS.has(entry.name)) walk(abs);
      } else if (entry.name.endsWith('.rs') && !EXCLUDED_FILES.has(entry.name)) {
        corpus[path.relative(repoRoot, abs)] = readFileSync(abs, 'utf8');
      }
    }
  }
  for (const root of CODE_ROOTS) walk(path.join(repoRoot, root));
  const out = path.join(appDir, 'src/lib/generated/code-corpus.json');
  mkdirSync(path.dirname(out), {recursive: true});
  writeFileSync(out, JSON.stringify(corpus));
}

writeCodeCorpus();

/** @type {import('next').NextConfig} */
const nextConfig = {
  reactStrictMode: true,

  async redirects() {
    return [
      // spec.bittensor.com: the Polkadot Vault onboarding shortcut — lands on
      // the guide section hosting the one-time chain-specs QR.
      {
        source: '/:path*',
        has: [{type: 'host', value: 'spec.bittensor.com'}],
        destination: 'https://www.bittensor.com/docs/guides/vault',
        permanent: false,
      },
      {
        source: '/documentation/:path*',
        destination: '/docs',
        permanent: true,
      },
      // Earlier collateral-era specs were squashed into the v436 release page;
      // v432 landed in the v431 monorepo release notes.
      {
        source: '/releases/v432-upgrade',
        destination: '/releases/v431-upgrade',
        permanent: true,
      },
      {
        source: '/releases/v434-upgrade',
        destination: '/releases/v436-upgrade',
        permanent: true,
      },
      {
        source: '/releases/v435-upgrade',
        destination: '/releases/v436-upgrade',
        permanent: true,
      },
      {
        source: '/scan/:path*',
        destination: 'https://taostats.io',
        permanent: true,
      },
      {
        source: '/scan',
        destination: 'https://taostats.io',
        permanent: true,
      },
    ];
  },

  transpilePackages: ['@raofoundation/ui'],

  webpack(config) {
    config.module.rules.push({
      test: /\.glsl/,
      type: 'asset/source',
    });

    config.externals.push({
      'utf-8-validate': 'commonjs utf-8-validate',
      bufferutil: 'commonjs bufferutil',
    });

    // Grab the existing rule that handles SVG imports
    const fileLoaderRule = config.module.rules.find((rule) => rule.test?.test?.('.svg'));

    config.module.rules.push(
      // Reapply the existing rule, but only for svg imports ending in ?url
      {
        ...fileLoaderRule,
        test: /\.svg$/i,
        resourceQuery: /url/, // *.svg?url
      },
      // Convert all other *.svg imports to React components
      {
        test: /\.svg$/i,
        issuer: fileLoaderRule.issuer,
        resourceQuery: {not: [...fileLoaderRule.resourceQuery.not, /url/]}, // exclude if *.svg?url
        use: ['@svgr/webpack'],
      },
    );

    // Modify the file loader rule to ignore *.svg, since we have it handled now.
    fileLoaderRule.exclude = /\.svg$/i;

    return config;
  },
};

export default withMDX(nextConfig);
