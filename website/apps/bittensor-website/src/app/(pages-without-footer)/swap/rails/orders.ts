/**
 * Orders, as the portal sends them to the hub. The page builds one only to ask the hub
 * for a quote; the contracts build the ones that count.
 */

import {AbiCoder, ZeroAddress, zeroPadValue} from 'ethers';

/** What an order pays with. */
export type Source = {kind: 'stable'} | {kind: 'tao'} | {kind: 'alpha'; netuid: number};

/** What an order asks for. */
export type Target =
  | {kind: 'stable'; to: string}
  | {kind: 'tao'; to: string}
  | {kind: 'alpha'; netuid: number; to: string}
  | {kind: 'credits'; netuid: number; service: string; beneficiary: string};

/** The `Wire.Destination` struct of the contracts. */
export type Destination = {kind: number; netuid: number; recipient: string; beneficiary: string};

const ORDER_VERSION = 1;

function sourceCode(source: Source): [number, number] {
  switch (source.kind) {
    case 'stable':
      return [0, 0];
    case 'tao':
      return [1, 0];
    case 'alpha':
      return [2, source.netuid];
    default: {
      const unknown: never = source;
      throw new Error(`unknown source ${JSON.stringify(unknown)}`);
    }
  }
}

export function destinationOf(target: Target): Destination {
  switch (target.kind) {
    case 'stable':
      return {kind: 0, netuid: 0, recipient: zeroPadValue(target.to, 32), beneficiary: ZeroAddress};
    case 'tao':
      return {kind: 1, netuid: 0, recipient: zeroPadValue(target.to, 32), beneficiary: ZeroAddress};
    case 'alpha':
      return {
        kind: 2,
        netuid: target.netuid,
        recipient: zeroPadValue(target.to, 32),
        beneficiary: ZeroAddress,
      };
    case 'credits':
      return {
        kind: 3,
        netuid: target.netuid,
        recipient: target.service,
        beneficiary: target.beneficiary,
      };
    default: {
      const unknown: never = target;
      throw new Error(`unknown target ${JSON.stringify(unknown)}`);
    }
  }
}

/** The twelve words of an order of `amount` (9 decimals), for the hub's `quote`. */
export function encodeOrder(source: Source, amount: bigint, target: Target): string {
  const [sourceKind, sourceNetuid] = sourceCode(source);
  const destination = destinationOf(target);
  return AbiCoder.defaultAbiCoder().encode(
    [
      'uint8',
      'uint64',
      'address',
      'uint8',
      'uint16',
      'uint64',
      'uint8',
      'uint16',
      'bytes32',
      'address',
      'uint64',
      'uint64',
    ],
    [
      ORDER_VERSION,
      0,
      ZeroAddress,
      sourceKind,
      sourceNetuid,
      amount,
      destination.kind,
      destination.netuid,
      destination.recipient,
      destination.beneficiary,
      0,
      0,
    ],
  );
}

/** Why the hub would refund an order, in words. */
export function explainRefusal(reason: string): string {
  const reasons: Record<string, string> = {
    AmountTooLow: 'The amount is too small to settle.',
    SlippageExceeded: 'The price moved past your limit.',
    SlippageTooHigh: 'The amount is too large for the subnet pool right now.',
    MarketCapExceeded: 'This token is at its cap for now.',
    TaoCapExceeded: 'Canonical TAO on this chain is at its cap for now.',
    MarketClosed: 'This subnet was dissolved. Its token can be sold, not bought.',
    MarketSettling: 'This subnet is being dissolved. Try again when that is done.',
    MarketUnknown: 'This subnet has no token.',
    TokenNotRegistered: 'This token is not listed on this chain.',
    TwapUnavailable: 'The pool is too new to price credits. Try again later.',
    SwapTooLarge: 'The amount is too large for the pool right now.',
    SwapRateLimited: 'The pool has reached its flow limit. Try again in a few minutes.',
    PoolDisabled: 'The pool is paused.',
    ReserveTooLow: 'The pool cannot fill this amount.',
    NoLiquidity: 'The pool has no liquidity yet.',
    Unbacked: 'There is not that much of this token on this chain.',
    BridgeDisabled: 'The bridge is paused.',
    MintCapExceeded: 'The bridge is at its cap for now.',
    MintRateLimited: 'The bridge has reached its rate limit. Try again in a few minutes.',
    ServiceDoesNotExist: 'This service does not take credits.',
    TaoUsdPriceUnavailable: 'The TAO price feed is stale. Try again in a few minutes.',
    InsufficientLiquidity: 'The subnet pool cannot fill this amount.',
  };
  return reasons[reason] ?? `The hub would refund this order (${reason || 'no reason given'}).`;
}
