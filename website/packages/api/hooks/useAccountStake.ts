import {useChainStore} from './useChainStore';

export type NeuronLite = {
  stake: [string, number][];
};

export type ApiRpc = {
  neuronInfo: {
    getNeuronsLite: (netuid: number) => Promise<Uint8Array>;
  };
};

const isApiRpc = (x: any): x is ApiRpc => {
  return typeof x.neuronInfo === 'object' && typeof x.neuronInfo.getNeuronsLite === 'function';
};

export const useAccountStake = async (address: string) => {
  let api = useChainStore.getState().api;

  if (!api || !isApiRpc(api.rpc)) return {stake: 0};

  const rawStake = await api.query['subtensorModule']['totalColdkeyStake'](address);
  const stake = Number(rawStake.toString()) ?? 0;

  return {stake};
};
