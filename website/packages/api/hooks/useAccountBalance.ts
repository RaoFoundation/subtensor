import {useChainStore} from './useChainStore';

export const useAccountBalance = async (address: string) => {
  let api = useChainStore.getState().api;

  const data = await api?.query.system.account(address);

  const balance = isPolkadotAccountObject(data) ? data?.data?.free?.toNumber() || 0 : 0;
  const free = isPolkadotAccountObject(data) ? data?.data?.free || undefined : undefined;

  return {balance, free};
};

export type PolkadotAccountObject = {
  data: {
    free: {
      toNumber: () => number;
    };
  };
};

export const isPolkadotAccountObject = (data: any): data is PolkadotAccountObject => {
  return typeof data?.data?.free?.toNumber === 'function';
};
