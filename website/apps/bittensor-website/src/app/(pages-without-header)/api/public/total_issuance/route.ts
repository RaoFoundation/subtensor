import {NextResponse} from 'next/server';
import {getAllChainStats} from '../../chainStats';

export const dynamic = 'force-dynamic';

export async function GET() {
  const data = await getAllChainStats();
  return NextResponse.json(data.total_issuance, {
    headers: {
      'Cache-Control': 'public, s-maxage=10, stale-while-revalidate',
    },
  });
}
