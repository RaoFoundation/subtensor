import type {ApiPromise} from '@polkadot/api';
import {create} from 'zustand';

export type ChainStore = {
  api: ApiPromise | null;
  setApi: (api: ApiPromise | null) => void;
};

export const useChainStore = create<ChainStore>()((set) => ({
  api: null,

  setApi: (api: ApiPromise | null) => {
    set({api});
  },
}));
