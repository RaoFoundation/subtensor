import {create} from 'zustand';

export type UIStore = {
  isVisible: boolean;
  toggle: () => void;
};

export const useHamburgerMenuStore = create<UIStore>((set, get) => ({
  isVisible: false,
  toggle: () => {
    const current = get().isVisible;
    set({isVisible: !current});
  },
}));
