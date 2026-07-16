import { source } from '@/lib/source';
import { llms } from 'fumadocs-core/source';
import { buildCommit, CODE_ROOTS } from '@/lib/code';
import { codeRoute } from '@/lib/shared';

export const revalidate = false;

function codeSection(): string {
  const commit = buildCommit();
  return [
    '## Chain source code',
    '',
    'The Rust that runs on the chain (pallets, runtime, primitives — no tests/mocks/benchmarks)' +
      (commit ? `, at commit ${commit}.` : '.'),
    '',
    `- [Index of every file (JSON)](${codeRoute}/index.json)`,
    `- Browse: ${codeRoute}/<repo-path> with #L<n> line anchors, e.g. ${codeRoute}/pallets/subtensor/src/coinbase/run_coinbase.rs`,
    `- Plain text: ${codeRoute}/raw/<repo-path>`,
    `- Roots: ${CODE_ROOTS.join(', ')}`,
    '',
  ].join('\n');
}

export function GET() {
  return new Response(`${llms(source).index()}\n${codeSection()}`);
}
