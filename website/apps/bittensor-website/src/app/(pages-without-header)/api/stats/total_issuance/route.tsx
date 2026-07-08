import {getChainApiHandler} from '@raofoundation/api';
import {WsProvider} from '@polkadot/api';
import {encodeAddress} from '@polkadot/util-crypto';
import {NextResponse} from 'next/server';
import {API_ENDPOINT} from '../../config';

export const dynamic = 'force-dynamic';

export type NeuronLite = {
  stake: [String, Number][];
};

export type ApiRpc = {
  neuronInfo: {
    getNeuronsLite: (netuid: number) => Promise<Uint8Array>;
  };
};

const isApiRpc = (x: any): x is ApiRpc => {
  return typeof x.neuronInfo === 'object' && typeof x.neuronInfo.getNeuronsLite === 'function';
};

const isNeuronLiteArray = (x: any): x is NeuronLite[] => {
  return x.every((y: any) => {
    return typeof y.stake === 'object';
  });
};

export async function GET() {
  const time = new Date().getTime();
  const provider = new WsProvider(API_ENDPOINT);
  const api = await getChainApiHandler(provider);

  try {
    if (!api || !isApiRpc(api.rpc)) {
      return NextResponse.json({error: 'Cannot read values from the chain'});
    }

    let total_stake = 0;

    let uniqueNeurons = new Set();
    const stakeMap: Record<string, number> = {};
    let netuids = [1, 3, 11, 21];

    for (const netuid of netuids) {
      const resultBytes = await api.rpc.neuronInfo.getNeuronsLite(netuid);
      const result = api.createType('Vec<NeuronInfoLite>', resultBytes);
      const neurons = result?.toJSON();

      if (!isNeuronLiteArray(neurons)) {
        return NextResponse.json({error: 'Cannot read values from the chain'});
      }

      neurons.forEach((item: any) => {
        if (uniqueNeurons.has(item.hotkey)) {
          return;
        }
        uniqueNeurons.add(item.hotkey);
        item.stake.forEach((stake: [string, number]) => {
          const [key, value] = stake;
          total_stake += value;
          if (stakeMap[key]) {
            stakeMap[key] += value;
          } else {
            stakeMap[key] = value;
          }
        });
      });
    }

    const limit = 1000;
    const result = [];
    const balances: {
      account: string;
      stake: number;
      free: number;
      total: number;
    }[] = [];
    let last_key = '';
    let accounts = 0;
    let total_free = 0;
    let total_reserved = 0;
    let total_locked = 0;

    while (true) {
      let query = await api.query.system.account.entriesPaged({
        args: [],
        pageSize: limit,
        startKey: last_key,
      });

      if (query.length == 0) {
        break;
      }

      for (const user of query) {
        const account_id = encodeAddress(user[0].slice(-32));
        const stake = stakeMap[account_id] || 0;
        // @ts-ignore
        const balance = user[1].data.free.toNumber();
        total_free += stake + balance;
        balances.push({
          account: account_id.toString(),
          stake: stake / 1000000000,
          free: balance / 1000000000,
          total: (stake + balance) / 1000000000,
        });
        last_key = user[0] as unknown as string;
        accounts += 1;
      }

      balances.sort((a, b) => b.total - a.total);
      balances.splice(20, balances.length - 20);
    }

    return NextResponse.json(
      {
        total_stake,
        accounts,
        balances,
        total_free: total_free / 1000000000,
        total_reserved: total_reserved / 1000000000,
        total_locked: total_locked / 1000000000,
        time: new Date().getTime() - time,
      },
      {
        headers: {
          'Cache-Control': 'public, s-maxage=10, stale-while-revalidate',
        },
      },
    );
  } catch {
    return NextResponse.json({error: 'Cannot read values from the chain'});
  }
}
