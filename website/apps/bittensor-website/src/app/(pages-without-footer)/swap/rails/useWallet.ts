'use client';

import {BrowserProvider, JsonRpcProvider, Wallet, type Eip1193Provider, type Signer} from 'ethers';
import {useCallback, useEffect, useState} from 'react';
import type {RailsConfig} from '../config';

type InjectedProvider = Eip1193Provider & {
  on?: (event: string, listener: (...args: unknown[]) => void) => void;
  removeListener?: (event: string, listener: (...args: unknown[]) => void) => void;
};

declare global {
  interface Window {
    ethereum?: InjectedProvider;
  }
}

export type WalletState = {
  account: string | null;
  signer: Signer | null;
  /** Whether a browser wallet is installed. */
  available: boolean;
  /** Whether the wallet is the local test account, not a browser wallet. */
  local: boolean;
  connecting: boolean;
  error: string | null;
  connect: () => Promise<void>;
  /** Local mode only: an account of the rig, so the page works without a wallet. */
  connectLocal: () => Promise<void>;
  /** Adds a token to the wallet's list. */
  watchToken: (address: string, symbol: string, decimals: number) => Promise<void>;
};

const chainIdHex = (config: RailsConfig) => `0x${config.spoke.chainId.toString(16)}`;

/** Puts the wallet on the spoke's chain, adding the chain first if the wallet lacks it. */
async function switchChain(provider: InjectedProvider, config: RailsConfig) {
  try {
    await provider.request({
      method: 'wallet_switchEthereumChain',
      params: [{chainId: chainIdHex(config)}],
    });
  } catch (error) {
    // 4902: the wallet does not know the chain.
    if ((error as {code?: number}).code !== 4902) throw error;
    await provider.request({
      method: 'wallet_addEthereumChain',
      params: [
        {
          chainId: chainIdHex(config),
          chainName: config.spoke.name,
          rpcUrls: [config.spoke.rpcUrl],
          nativeCurrency: config.spoke.nativeCurrency,
          blockExplorerUrls: config.spoke.explorer ? [config.spoke.explorer] : [],
        },
      ],
    });
  }
}

export function useWallet(config: RailsConfig): WalletState {
  const [account, setAccount] = useState<string | null>(null);
  const [signer, setSigner] = useState<Signer | null>(null);
  const [available, setAvailable] = useState(false);
  const [local, setLocal] = useState(false);
  const [connecting, setConnecting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setAvailable(typeof window !== 'undefined' && Boolean(window.ethereum));
  }, []);

  const adopt = useCallback(
    async (injected: InjectedProvider) => {
      await switchChain(injected, config);
      const next = await new BrowserProvider(injected).getSigner();
      setSigner(next);
      setAccount(await next.getAddress());
      setLocal(false);
    },
    [config],
  );

  const connect = useCallback(async () => {
    const injected = window.ethereum;
    if (!injected) {
      setError('No wallet found. Install MetaMask, then reload this page.');
      return;
    }
    setConnecting(true);
    setError(null);
    try {
      await injected.request({method: 'eth_requestAccounts'});
      await adopt(injected);
    } catch (cause) {
      setError(describe(cause));
    } finally {
      setConnecting(false);
    }
  }, [adopt]);

  const connectLocal = useCallback(async () => {
    if (!config.local.testKey) return;
    setError(null);
    const wallet = new Wallet(
      config.local.testKey,
      new JsonRpcProvider(config.spoke.rpcUrl, config.spoke.chainId, {staticNetwork: true}),
    );
    setSigner(wallet);
    setAccount(wallet.address);
    setLocal(true);
  }, [config]);

  // Follow the wallet when the person changes the account or the chain in it.
  useEffect(() => {
    const injected = typeof window === 'undefined' ? undefined : window.ethereum;
    if (!injected?.on || !account || local) return;
    const follow = () => {
      adopt(injected).catch((cause) => {
        setAccount(null);
        setSigner(null);
        setError(describe(cause));
      });
    };
    injected.on('accountsChanged', follow);
    injected.on('chainChanged', follow);
    return () => {
      injected.removeListener?.('accountsChanged', follow);
      injected.removeListener?.('chainChanged', follow);
    };
  }, [account, adopt, local]);

  const watchToken = useCallback(async (address: string, symbol: string, decimals: number) => {
    await window.ethereum?.request({
      method: 'wallet_watchAsset',
      params: {type: 'ERC20', options: {address, symbol, decimals}},
    });
  }, []);

  return {account, signer, available, local, connecting, error, connect, connectLocal, watchToken};
}

/** What went wrong, in the wallet's or the chain's own words. */
export function describe(cause: unknown): string {
  const error = cause as {shortMessage?: string; reason?: string; message?: string; code?: unknown};
  if (error.code === 4001 || error.code === 'ACTION_REJECTED') return 'You declined in the wallet.';
  return error.shortMessage ?? error.reason ?? error.message ?? 'Something went wrong.';
}
