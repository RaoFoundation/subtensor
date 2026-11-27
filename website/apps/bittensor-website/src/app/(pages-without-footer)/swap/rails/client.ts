/**
 * Everything the page reads from, and writes to, the two chains. Reads go through public
 * RPC endpoints; writes go through the wallet.
 */

import {Contract, Interface, JsonRpcProvider, type Signer} from 'ethers';
import type {RailsConfig} from '../config';
import {
  ALPHA_TOKEN_ABI,
  ERC20_ABI,
  FAUCET_ABI,
  PORTAL_ABI,
  RAILS_ABI,
  RAILS_ADDRESS,
  TOKEN_ABI,
  VAULT_ABI,
} from './abi';
import {destinationOf, encodeOrder, type Source, type Target} from './orders';
import {RATE_SCALE, STABLE_SCALE, ZERO} from './units';

/** A canonical token on the spoke. */
export type Token = {
  kind: 'tao' | 'alpha';
  /** The subnet of an alpha token, 0 for TAO. */
  netuid: number;
  address: string;
  symbol: string;
  name: string;
};

/** What an account holds of a token. */
export type Holding = {
  balance: bigint;
  /** For an alpha token: the alpha the tokens are worth now. */
  worthAlpha: bigint | null;
  /** For an alpha token: the alpha they were worth when the account got them. */
  principalAlpha: bigint | null;
};

export type Quote = {ok: true; amountOut: bigint} | {ok: false; reason: string};

/** What the hub did with an order. */
export type OrderStatus = 'pending' | 'settled' | 'refunded' | 'rejected' | 'skipped';

/** The portal carried out the hub's answer to an order. */
export type Answer = {amount: bigint; refund: boolean; hash: string};

export type Rails = {
  config: RailsConfig;
  spoke: JsonRpcProvider;
  hub: JsonRpcProvider;
  portal: Contract;
  stable: Contract;
  hubRails: Contract;
};

const PORTAL = new Interface(PORTAL_ABI);

const STATUSES: OrderStatus[] = ['pending', 'settled', 'refunded', 'rejected', 'skipped'];

export function openRails(config: RailsConfig): Rails | null {
  if (!config.contracts) return null;
  const spoke = new JsonRpcProvider(config.spoke.rpcUrl, config.spoke.chainId, {
    staticNetwork: true,
  });
  const hub = new JsonRpcProvider(config.hub.rpcUrl, undefined, {staticNetwork: true});
  return {
    config,
    spoke,
    hub,
    portal: new Contract(config.contracts.portal, PORTAL_ABI, spoke),
    stable: new Contract(config.contracts.stable, ERC20_ABI, spoke),
    hubRails: new Contract(RAILS_ADDRESS, RAILS_ABI, hub),
  };
}

/** Canonical TAO and the alpha token of every configured market that exists on the spoke. */
export async function loadTokens(rails: Rails): Promise<Token[]> {
  const describe = async (kind: Token['kind'], netuid: number, address: string) => {
    const token = new Contract(address, ERC20_ABI, rails.spoke);
    const [symbol, name] = await Promise.all([token.symbol(), token.name()]);
    return {kind, netuid, address, symbol, name} satisfies Token;
  };
  const tao = describe('tao', 0, await rails.portal.tao());
  const alphas = rails.config.markets.map(async (netuid) => {
    const address: string = await rails.portal.alphaTokens(netuid);
    return /^0x0+$/.test(address) ? null : describe('alpha', netuid, address);
  });
  const tokens = await Promise.all([tao, ...alphas]);
  return tokens.filter((token): token is Token => token !== null);
}

export async function stableBalance(rails: Rails, account: string): Promise<bigint> {
  return rails.stable.balanceOf(account);
}

export async function loadHolding(rails: Rails, token: Token, account: string): Promise<Holding> {
  if (token.kind === 'tao') {
    const balance: bigint = await new Contract(token.address, ERC20_ABI, rails.spoke).balanceOf(
      account,
    );
    return {balance, worthAlpha: null, principalAlpha: null};
  }
  const contract = new Contract(token.address, ALPHA_TOKEN_ABI, rails.spoke);
  const [balance, principal, reported, live] = await Promise.all([
    contract.balanceOf(account) as Promise<bigint>,
    contract.principalOf(account) as Promise<bigint>,
    contract.rate() as Promise<bigint>,
    // The live rate is on Bittensor. The token's own is what the hub last reported.
    (rails.hubRails.alphaRate(token.netuid) as Promise<bigint>).catch(() => ZERO),
  ]);
  const rate = live > ZERO ? live : reported;
  return {balance, worthAlpha: (balance * rate) / RATE_SCALE, principalAlpha: principal};
}

/** The credits that the rails have spent at a service for an address, in total. */
export async function creditsOf(
  rails: Rails,
  netuid: number,
  service: string,
  account: string,
): Promise<bigint> {
  return rails.hubRails.creditsOf(netuid, service, account);
}

/** What the hub would deliver for an order of `amount` (9 decimals) now. */
export async function quote(
  rails: Rails,
  source: Source,
  amount: bigint,
  target: Target,
): Promise<Quote> {
  const [ok, amountOut, reason] = await rails.hubRails.quote(
    rails.config.hub.bridge,
    encodeOrder(source, amount, target),
  );
  return ok ? {ok: true, amountOut} : {ok: false, reason};
}

/** The order that a transaction sent, from its receipt. */
function orderOf(logs: readonly {topics: readonly string[]; data: string}[]): bigint {
  for (const log of logs) {
    const parsed = PORTAL.parseLog({topics: [...log.topics], data: log.data});
    if (parsed?.name === 'OrderSent') return parsed.args.nonce as bigint;
  }
  throw new Error('The transaction sent no order.');
}

export type Sent = {nonce: bigint; hash: string; block: number};

/** Lets the vault take `amount` of the signer's stablecoin, if it may not already. */
export async function approve(rails: Rails, signer: Signer, amount: bigint): Promise<boolean> {
  const vault = rails.config.contracts!.vault;
  const stable = rails.stable.connect(signer) as Contract;
  const allowance: bigint = await stable.allowance(await signer.getAddress(), vault);
  if (allowance >= amount) return false;
  await (await stable.approve(vault, amount)).wait();
  return true;
}

function deadline(ttlSeconds: number): number {
  return Math.floor(Date.now() / 1000) + ttlSeconds;
}

/** Locks `amount` of the stablecoin (its own decimals) in the vault and orders `target`. */
export async function deposit(
  rails: Rails,
  signer: Signer,
  amount: bigint,
  target: Target,
  minOut: bigint,
  ttlSeconds: number,
): Promise<Sent> {
  const vault = new Contract(rails.config.contracts!.vault, VAULT_ABI, signer);
  const fee: bigint = await rails.portal.orderFee();
  const sent = await vault.deposit(amount, destinationOf(target), minOut, deadline(ttlSeconds), {
    value: fee,
  });
  const receipt = await sent.wait();
  return {nonce: orderOf(receipt.logs), hash: receipt.hash, block: receipt.blockNumber};
}

/** Burns `amount` of a canonical token and orders `target`. */
export async function sell(
  rails: Rails,
  signer: Signer,
  token: Token,
  amount: bigint,
  target: Target,
  minOut: bigint,
  ttlSeconds: number,
): Promise<Sent> {
  const contract = new Contract(token.address, TOKEN_ABI, signer);
  const fee: bigint = await rails.portal.orderFee();
  const sent = await contract.sell(amount, destinationOf(target), minOut, deadline(ttlSeconds), {
    value: fee,
  });
  const receipt = await sent.wait();
  return {nonce: orderOf(receipt.logs), hash: receipt.hash, block: receipt.blockNumber};
}

/** What the hub did with order `nonce`, and what the order delivered if it settled. */
export async function orderStatus(
  rails: Rails,
  nonce: bigint,
): Promise<{status: OrderStatus; amountOut: bigint}> {
  const [status, amountOut] = await rails.hubRails.receipt(rails.config.hub.bridge, nonce);
  return {status: STATUSES[Number(status)] ?? 'pending', amountOut};
}

/** The portal's payment in answer to order `nonce`, once it has arrived. */
export async function findAnswer(
  rails: Rails,
  nonce: bigint,
  fromBlock: number,
): Promise<Answer | null> {
  // A filter names every field of the event in order. `order` is the seventh.
  const logs = await rails.portal.queryFilter(
    rails.portal.filters.Credited(null, null, null, null, null, null, nonce),
    fromBlock,
  );
  for (const log of logs) {
    const parsed = PORTAL.parseLog({topics: [...log.topics], data: log.data});
    if (parsed?.args.answersOrder) {
      return {amount: parsed.args.amount, refund: parsed.args.refund, hash: log.transactionHash};
    }
  }
  return null;
}

/** Local mode: mints the test stablecoin to the signer. */
export async function faucet(rails: Rails, signer: Signer, amount: bigint): Promise<void> {
  const stable = new Contract(rails.config.contracts!.stable, FAUCET_ABI, signer);
  await (await stable.mint(await signer.getAddress(), amount)).wait();
}

/** The stablecoin's units as the hub counts them. */
export const toHubUnits = (stableAmount: bigint) => stableAmount * STABLE_SCALE;
