import { buildCommit, codeFiles } from '@/lib/code';
import { codeRoute } from '@/lib/shared';

// Machine-readable index of the on-chain source corpus: every file with its
// viewer, raw, and anchor URL conventions, stamped with the build commit.
export const dynamic = 'force-static';

export function GET() {
  const manifest = {
    commit: buildCommit(),
    note:
      'Rust source of the Bittensor chain (pallets, runtime, primitives; no tests/mocks/benchmarks). ' +
      `View at ${codeRoute}/<path> (line anchors: #L<n>), fetch plain text at ${codeRoute}/raw/<path>.`,
    files: codeFiles(),
  };
  return Response.json(manifest);
}
