'use client';

import {useCallback, useEffect, useRef, useState} from 'react';
import {
  creditsOf,
  findAnswer,
  loadHolding,
  loadTokens,
  orderStatus,
  quote,
  stableBalance,
  type Answer,
  type Holding,
  type OrderStatus,
  type Quote,
  type Rails,
  type Sent,
  type Token,
} from './client';
import type {Source, Target} from './orders';
import {ZERO} from './units';
import {describe} from './useWallet';

const REFRESH_MS = 6_000;
const QUOTE_DELAY_MS = 350;
const ORDER_POLL_MS = 2_500;

/** The canonical tokens on the spoke. `null` while they load. */
export function useTokens(rails: Rails | null): Token[] | null {
  const [tokens, setTokens] = useState<Token[] | null>(null);
  useEffect(() => {
    if (!rails) return;
    let cancelled = false;
    const load = () =>
      loadTokens(rails)
        .then((loaded) => !cancelled && setTokens(loaded))
        .catch(() => undefined);
    load();
    // A market that is listed later appears without a reload.
    const timer = setInterval(load, 5 * REFRESH_MS);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [rails]);
  return tokens;
}

export type Portfolio = {
  stable: bigint | null;
  holdings: Record<string, Holding>;
  /** Credits bought for the account, by service account. */
  credits: Record<string, bigint>;
  refresh: () => void;
};

/** What the account holds, refreshed while the page is open. */
export function usePortfolio(rails: Rails | null, account: string | null, tokens: Token[] | null) {
  const [stable, setStable] = useState<bigint | null>(null);
  const [holdings, setHoldings] = useState<Record<string, Holding>>({});
  const [credits, setCredits] = useState<Record<string, bigint>>({});
  const [tick, setTick] = useState(0);
  const refresh = useCallback(() => setTick((value) => value + 1), []);

  useEffect(() => {
    if (!rails || !account) {
      setStable(null);
      setHoldings({});
      setCredits({});
      return;
    }
    let cancelled = false;
    const load = async () => {
      const [balance, held, spent] = await Promise.all([
        stableBalance(rails, account),
        Promise.all((tokens ?? []).map((token) => loadHolding(rails, token, account))),
        Promise.all(
          rails.config.services.map((service) =>
            creditsOf(rails, service.netuid, service.account, account).catch(() => ZERO),
          ),
        ),
      ]);
      if (cancelled) return;
      setStable(balance);
      setHoldings(Object.fromEntries((tokens ?? []).map((token, i) => [token.address, held[i]])));
      setCredits(
        Object.fromEntries(rails.config.services.map((service, i) => [service.account, spent[i]])),
      );
    };
    load().catch(() => undefined);
    const timer = setInterval(() => load().catch(() => undefined), REFRESH_MS);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [rails, account, tokens, tick]);

  return {stable, holdings, credits, refresh} satisfies Portfolio;
}

export type QuoteState =
  | {state: 'idle'}
  | {state: 'loading'}
  | {state: 'ready'; amountOut: bigint}
  | {state: 'refused'; reason: string}
  | {state: 'error'; message: string};

/** The hub's quote for an order, asked again a moment after the order changes. */
export function useQuote(
  rails: Rails | null,
  source: Source | null,
  amount: bigint | null,
  target: Target | null,
): QuoteState {
  const [state, setState] = useState<QuoteState>({state: 'idle'});
  // The order as text: an effect compares its dependencies by identity.
  const key =
    source && target && amount ? JSON.stringify([source, amount.toString(), target]) : null;

  useEffect(() => {
    if (!rails || !key || !source || !target || !amount) {
      setState({state: 'idle'});
      return;
    }
    let cancelled = false;
    setState({state: 'loading'});
    const timer = setTimeout(() => {
      quote(rails, source, amount, target)
        .then((result: Quote) => {
          if (cancelled) return;
          setState(
            result.ok
              ? {state: 'ready', amountOut: result.amountOut}
              : {state: 'refused', reason: result.reason},
          );
        })
        .catch((cause) => !cancelled && setState({state: 'error', message: describe(cause)}));
    }, QUOTE_DELAY_MS);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
    // `key` stands for source, amount and target.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rails, key]);

  return state;
}

/** Where an order is on its way. */
export type OrderProgress = {
  sent: Sent;
  /** Whether a settled order pays something out on the spoke. Credits do not. */
  paysOut: boolean;
  status: OrderStatus;
  /** What the hub delivered, once it settled the order. */
  amountOut: bigint | null;
  /** The portal's payment, once it arrived. */
  answer: Answer | null;
  startedAt: number;
  finishedAt: number | null;
};

/** Follows one order from the wallet to the hub and back. */
export function useOrder(rails: Rails | null, onFinished: () => void) {
  const [progress, setProgress] = useState<OrderProgress | null>(null);
  const finished = useRef(onFinished);
  finished.current = onFinished;

  const track = useCallback((sent: Sent, paysOut: boolean) => {
    setProgress({
      sent,
      paysOut,
      status: 'pending',
      amountOut: null,
      answer: null,
      startedAt: Date.now(),
      finishedAt: null,
    });
  }, []);

  const nonce = progress?.sent.nonce ?? null;
  const done = progress?.finishedAt != null;

  useEffect(() => {
    if (!rails || nonce === null || done) return;
    let cancelled = false;
    const poll = async () => {
      const current = await orderStatus(rails, nonce);
      if (cancelled) return;
      setProgress((previous) => {
        if (!previous || previous.sent.nonce !== nonce) return previous;
        return {...previous, status: current.status, amountOut: current.amountOut};
      });
      if (current.status === 'pending') return;
      // A settled order that pays out, and a refund, both end with a payment on the spoke.
      const awaitsPayment =
        current.status === 'refunded' || (current.status === 'settled' && progress?.paysOut);
      const answer = awaitsPayment
        ? await findAnswer(rails, nonce, progress?.sent.block ?? 0)
        : null;
      if (cancelled || (awaitsPayment && !answer)) return;
      setProgress((previous) =>
        previous && previous.sent.nonce === nonce
          ? {...previous, answer, finishedAt: Date.now()}
          : previous,
      );
      finished.current();
    };
    poll().catch(() => undefined);
    const timer = setInterval(() => poll().catch(() => undefined), ORDER_POLL_MS);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
    // The order's own fields are read through `progress` but do not restart the polling.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rails, nonce, done]);

  return {progress, track, clear: useCallback(() => setProgress(null), [])};
}

/** Seconds since `since`, counted once a second until `until`. */
export function useElapsed(since: number | null, until: number | null): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (since === null || until !== null) return;
    const timer = setInterval(() => setNow(Date.now()), 1_000);
    return () => clearInterval(timer);
  }, [since, until]);
  if (since === null) return 0;
  return Math.max(0, Math.round(((until ?? now) - since) / 1_000));
}
