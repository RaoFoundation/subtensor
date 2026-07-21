import { source } from '@/lib/source';
import { llms } from 'fumadocs-core/source';
import { buildCommit, CODE_ROOTS } from '@/lib/code';
import { chainRepoUrl, codeRoute, docsContentRoute, siteUrl } from '@/lib/shared';

export const revalidate = false;

function searchSection(): string {
  return [
    '## Searching these docs',
    '',
    'Everything below is plain text over HTTP, so you can grep it as if it were local:',
    '',
    `- Full docs corpus (every page in one file): \`curl -s ${siteUrl}/llms-full.txt | rg -n '<pattern>'\``,
    `- One page as markdown: ${siteUrl}${docsContentRoute}/<slug>/content.md, e.g. ${siteUrl}${docsContentRoute}/quickstart/content.md`,
    `- Chain source: \`curl -s ${siteUrl}${codeRoute}/index.json\` lists every file path, then \`curl -s ${siteUrl}${codeRoute}/raw/<repo-path> | rg -n '<pattern>'\``,
    `- For heavy exploration, clone the repo (docs live in docs/): \`git clone --depth 1 ${chainRepoUrl}\``,
    '',
  ].join('\n');
}

function codeSection(): string {
  const commit = buildCommit();
  return [
    '## Chain source code',
    '',
    'The Rust that runs on the chain (pallets, runtime, primitives — no tests/mocks/benchmarks)' +
      (commit ? `, at commit ${commit}.` : '.'),
    '',
    `- [Index of every file (JSON)](${codeRoute}/index.json)`,
    `- Browse: ${codeRoute}/<repo-path> with #L<n> or #L<n>-L<m> line anchors, e.g. ${codeRoute}/pallets/subtensor/src/coinbase/run_coinbase.rs`,
    `- Plain text: ${codeRoute}/raw/<repo-path>`,
    `- Roots: ${CODE_ROOTS.join(', ')}`,
    '',
  ].join('\n');
}

export function GET() {
  return new Response(`${llms(source).index()}\n${searchSection()}\n${codeSection()}`);
}
