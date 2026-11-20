import { buildCommit, searchCode } from '@/lib/code';
import corpus from '@/lib/generated/code-corpus.json';
import { codeRoute } from '@/lib/shared';

export const dynamic = 'force-dynamic';

/**
 * Agent-facing content search over the on-chain Rust corpus.
 * GET /code/search.json?q=<literal>&limit=20
 *
 * The corpus is a build-time snapshot (next.config.mjs) bundled into this
 * function — the Vercel checkout is not available at request time.
 */
export function GET(req: Request) {
  const url = new URL(req.url);
  const q = url.searchParams.get('q')?.trim() ?? '';
  const limit = Number(url.searchParams.get('limit') ?? '20');

  if (q.length < 2) {
    return Response.json(
      {
        error: 'pass ?q= with at least 2 characters (literal substring match)',
        example: `${codeRoute}/search.json?q=do_move_stake`,
      },
      { status: 400 },
    );
  }

  const results = searchCode(q, Number.isFinite(limit) ? limit : 20, corpus);
  return Response.json({
    commit: buildCommit(),
    q,
    count: results.length,
    results,
  });
}
