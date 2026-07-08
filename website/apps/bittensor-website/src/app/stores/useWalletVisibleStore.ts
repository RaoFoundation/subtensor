import {create} from 'zustand';

export type UIStore = {
  isVisible: boolean;
  open: () => void;
  close: () => void;
};

export const useWalletVisibleStore = create<UIStore>((set, get) => ({
  isVisible: false,
  open: () => {
    set({isVisible: true});
  },
  close: () => {
    set({isVisible: false});
  },
}));
