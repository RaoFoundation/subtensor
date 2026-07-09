import {NextResponse} from 'next/server';
import {CHAIN_STATS_ENDPOINT} from '../../config';

export const dynamic = 'force-dynamic';

type ChainStatusResponse = {
  total_issuance: number;
};

const isChainStatusResponse = (value: any): value is ChainStatusResponse => {
  return typeof value === 'object' && value !== null && typeof value.total_issuance === 'number';
};

const getTotalIssuance = async () => {
  const response = await fetch(CHAIN_STATS_ENDPOINT + '?' + Math.random());
  const data = await response.json();
  const rawResponse = data?.response;

  if (!isChainStatusResponse(rawResponse)) {
    throw new Error('Invalid response');
  }

  return rawResponse.total_issuance;
};

export async function GET() {
  const totalIssuance = await getTotalIssuance();
  return NextResponse.json(totalIssuance, {
    headers: {
      'Cache-Control': 'public, s-maxage=10, stale-while-revalidate',
    },
  });
}
