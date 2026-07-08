import {getChainApiHandler} from '@raofoundation/api';
import {WsProvider} from '@polkadot/api';
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
  const provider = new WsProvider(API_ENDPOINT);

  //@ts-ignore
  const api = await getChainApiHandler(provider);

  try {
    if (!api || !isApiRpc(api.rpc)) {
      return NextResponse.json({error: 'Cannot read values from the chain'});
    }

    // const raw_issuance = await api.query.subtensorModule.totalIssuance();
    const raw_stake = await api.query.subtensorModule.stake.entries();
    let total_stake = raw_stake.reduce((sum: number, num: any) => {
      // console.log({num: num.length});
      return sum + num[1].toNumber();
    }, 0);

    const raw_stake2 = await api.query.subtensorModule.totalIssuance();
    const total_stake2 = api.createType('u64', raw_stake2).toNumber() / 1000000000;
    // const total_issuance = api.createType('u64', raw_issuance).toNumber() / 1000000000;
    // let uniqueNeurons = new Set();
    // let total_stake = 0
    // let netuids = [1, 3, 11, 21];

    // for (const netuid of netuids) {
    //   const resultBytes = await api.rpc.neuronInfo.getNeuronsLite(netuid);
    //   const result = api.createType('Vec<NeuronInfoLite>', resultBytes);
    //   const neurons = result?.toJSON();

    //   if (!isNeuronLiteArray(neurons)) {
    //     return NextResponse.json({error: 'Cannot read values from the chain'});
    //   }

    //   neurons.forEach((item: any) => {
    //     if (uniqueNeurons.has(item.hotkey)) {
    //       return;
    //     }
    //     uniqueNeurons.add(item.hotkey);
    //     total_stake +=
    //       item.stake.reduce((sum: number, num: number[]) => sum + num[1], 0) / 1000000000;
    //   });
    // }

    return NextResponse.json(
      {total_stake: total_stake / 1000000000, total_stake2},
      {
        headers: {
          'Cache-Control': 'public, s-maxage=10, stale-while-revalidate',
        },
      },
    );
  } catch (e) {
    return NextResponse.json({error: 'Cannot read values from the chain', e: (e as any).message});
  }
}
