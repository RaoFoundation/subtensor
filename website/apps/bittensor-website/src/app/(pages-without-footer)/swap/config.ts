/**
 * Where the swap page finds the canonical rails: contract addresses and public RPC
 * endpoints. The page has no backend. Everything it shows, it reads from these chains.
 *
 * `mainnet` is the configuration that ships. `local` is the rig that
 * `just rails-up` starts: the rig writes its addresses into `.env.local`, and the build
 * reads them from there.
 */

export type Address = `0x${string}`;

/** A service that takes credits on a subnet. */
export type Service = {
  netuid: number;
  /** What the buyer knows the service as, for example "LIUM". The page adds "credits". */
  name: string;
  /** The service's account on Bittensor: 32 bytes, hex. */
  account: `0x${string}`;
};

export type RailsConfig = {
  mode: 'mainnet' | 'local';
  /** The hub: Bittensor's EVM, read-only. */
  hub: {
    name: string;
    rpcUrl: string;
    /** The id the hub knows this spoke's portal by. */
    bridge: number;
  };
  /** The spoke: the chain the wallet is on. */
  spoke: {
    name: string;
    chainId: number;
    rpcUrl: string;
    /** A block explorer's base URL, without a trailing slash. */
    explorer: string | null;
    nativeCurrency: {name: string; symbol: string; decimals: number};
  };
  /** The rails' contracts on the spoke. `null` until they are deployed. */
  contracts: {vault: Address; portal: Address; stable: Address} | null;
  /** The subnets whose alpha token the page offers, in the order it shows them. */
  markets: number[];
  services: Service[];
  /** Local mode only: a free mint of the test stablecoin, and an account that needs no wallet. */
  local: {faucet: boolean; testKey: `0x${string}` | null};
};

const mainnet: RailsConfig = {
  mode: 'mainnet',
  hub: {
    name: 'Bittensor',
    rpcUrl: 'https://lite.chain.opentensor.ai',
    bridge: 8453,
  },
  spoke: {
    name: 'Base',
    chainId: 8453,
    rpcUrl: 'https://mainnet.base.org',
    explorer: 'https://basescan.org',
    nativeCurrency: {name: 'Ether', symbol: 'ETH', decimals: 18},
  },
  // Filled in when the portal and the vault are deployed on Base.
  contracts: null,
  markets: [],
  services: [],
  local: {faucet: false, testKey: null},
};

/** The rig's configuration, as `scripts/rails/up.sh` wrote it, or `null`. */
function localConfig(): RailsConfig | null {
  const raw = process.env.NEXT_PUBLIC_RAILS_LOCAL;
  if (!raw) return null;
  try {
    return {...(JSON.parse(raw) as RailsConfig), mode: 'local'};
  } catch {
    return null;
  }
}

export const railsConfig: RailsConfig =
  (process.env.NEXT_PUBLIC_RAILS_ENV === 'local' ? localConfig() : null) ?? mainnet;

/** How long an order may wait on the hub before it is refunded, in seconds. */
export const ORDER_TTL_SECONDS = 30 * 60;

/** The price movement a buyer accepts by default, in basis points. */
export const DEFAULT_SLIPPAGE_BPS = 100;
