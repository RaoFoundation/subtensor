'use client';

import clsx from 'clsx';
import {useCallback, useEffect, useMemo, useState, type ReactNode} from 'react';
import {DEFAULT_SLIPPAGE_BPS, ORDER_TTL_SECONDS, railsConfig, type Service} from './config';
import {
  approve,
  deposit,
  faucet,
  openRails,
  sell,
  toHubUnits,
  type Holding,
  type Token,
} from './rails/client';
import {
  useElapsed,
  useOrder,
  usePortfolio,
  useQuote,
  useTokens,
  type OrderProgress,
  type QuoteState,
} from './rails/hooks';
import {explainRefusal, type Source, type Target} from './rails/orders';
import {
  exactAmount,
  formatAmount,
  formatUnits9,
  lessBps,
  parseAmount,
  percentChange,
  STABLE_DECIMALS,
  ZERO,
} from './rails/units';
import {describe, useWallet} from './rails/useWallet';
import styles from './page.module.css';

const STABLE = 'USDC';
const FAUCET_AMOUNT = BigInt(1_000) * BigInt(10) ** BigInt(STABLE_DECIMALS);

/** What the person is here for. */
type Mode = 'token' | 'tao' | 'credits';
type Side = 'buy' | 'sell';

type LogLine = {at: number; text: string; tone: 'plain' | 'good' | 'bad'};

/** One trade, as the form describes it: what is paid, and what the hub is asked for. */
type Trade = {
  source: Source;
  target: Target;
  /** What is paid, in the hub's 9 decimals. */
  amountHub: bigint;
  /** What is paid, in the units the wallet moves. */
  amountWallet: bigint;
  payUnit: string;
  receiveUnit: string;
  /** How to show what is received: the hub answers in 9 decimals. */
  receiveDigits: number;
  route: string;
};

const short = (address: string) => `${address.slice(0, 6)}…${address.slice(-4)}`;

export default function SwapPage() {
  const rails = useMemo(() => openRails(railsConfig), []);
  const wallet = useWallet(railsConfig);
  const tokens = useTokens(rails);
  const portfolio = usePortfolio(rails, wallet.account, tokens);
  const order = useOrder(rails, portfolio.refresh);

  const [mode, setMode] = useState<Mode>('token');
  const [side, setSide] = useState<Side>('buy');
  const [netuid, setNetuid] = useState<number | null>(null);
  const [serviceAccount, setServiceAccount] = useState<string | null>(null);
  const [amountText, setAmountText] = useState('');
  const [busy, setBusy] = useState<string | null>(null);
  const [log, setLog] = useState<LogLine[]>([]);

  const say = useCallback((text: string, tone: LogLine['tone'] = 'plain') => {
    setLog((lines) => [{at: Date.now(), text, tone}, ...lines].slice(0, 40));
  }, []);

  const alphaTokens = useMemo(() => (tokens ?? []).filter((t) => t.kind === 'alpha'), [tokens]);
  const taoToken = useMemo(() => (tokens ?? []).find((t) => t.kind === 'tao') ?? null, [tokens]);
  const services = railsConfig.services;

  // Start on the first market and the first service.
  useEffect(() => {
    if (netuid === null && alphaTokens.length > 0) setNetuid(alphaTokens[0].netuid);
  }, [alphaTokens, netuid]);
  useEffect(() => {
    if (serviceAccount === null && services.length > 0) setServiceAccount(services[0].account);
  }, [services, serviceAccount]);

  const token: Token | null =
    mode === 'tao' ? taoToken : (alphaTokens.find((t) => t.netuid === netuid) ?? null);
  const service: Service | null = services.find((s) => s.account === serviceAccount) ?? null;
  const holding: Holding | null = token ? (portfolio.holdings[token.address] ?? null) : null;
  const effectiveSide: Side = mode === 'credits' ? 'buy' : side;

  const trade = useMemo(
    () => describeTrade(mode, effectiveSide, token, service, wallet.account, amountText),
    [mode, effectiveSide, token, service, wallet.account, amountText],
  );
  const quote = useQuote(
    rails,
    trade?.source ?? null,
    trade?.amountHub ?? null,
    trade?.target ?? null,
  );

  const available = effectiveSide === 'buy' ? portfolio.stable : holding ? holding.balance : null;
  const exceeds = trade !== null && available !== null && trade.amountWallet > available;

  const submit = async () => {
    if (!rails || !wallet.signer || !trade || quote.state !== 'ready') return;
    const minOut = lessBps(quote.amountOut, DEFAULT_SLIPPAGE_BPS);
    try {
      let sent;
      if (trade.source.kind === 'stable') {
        setBusy('Approve in your wallet');
        if (await approve(rails, wallet.signer, trade.amountWallet)) {
          say(`The vault may take ${formatAmount(trade.amountWallet, STABLE_DECIMALS)} ${STABLE}.`);
        }
        setBusy('Confirm in your wallet');
        sent = await deposit(
          rails,
          wallet.signer,
          trade.amountWallet,
          trade.target,
          minOut,
          ORDER_TTL_SECONDS,
        );
      } else if (token) {
        setBusy('Confirm in your wallet');
        sent = await sell(
          rails,
          wallet.signer,
          token,
          trade.amountWallet,
          trade.target,
          minOut,
          ORDER_TTL_SECONDS,
        );
      } else {
        return;
      }
      say(`Order ${sent.nonce} is on its way to Bittensor.`, 'good');
      order.track(sent, trade.target.kind !== 'credits');
      setAmountText('');
      portfolio.refresh();
    } catch (cause) {
      say(describe(cause), 'bad');
    } finally {
      setBusy(null);
    }
  };

  const mint = async () => {
    if (!rails || !wallet.signer || !wallet.account) return;
    setBusy('Minting test USDC');
    try {
      // The rig's chain hands out gas on request.
      await rails.spoke.send('anvil_setBalance', [wallet.account, '0x8AC7230489E80000']);
      await faucet(rails, wallet.signer, FAUCET_AMOUNT);
      say(`${formatAmount(FAUCET_AMOUNT, STABLE_DECIMALS)} test ${STABLE} minted.`, 'good');
      portfolio.refresh();
    } catch (cause) {
      say(describe(cause), 'bad');
    } finally {
      setBusy(null);
    }
  };

  const useMax = () => {
    if (available === null) return;
    setAmountText(exactAmount(available, effectiveSide === 'buy' ? STABLE_DECIMALS : 9));
  };

  const connected = Boolean(wallet.account);

  return (
    <div className={styles.page}>
      <header className={styles.intro}>
        <p className={styles.eyebrow}>Canonical rails · {railsConfig.spoke.name}</p>
        <h1 className={styles.title}>Bittensor, in the wallet you already have</h1>
        <p className={styles.lede}>
          Pay with {STABLE} on {railsConfig.spoke.name}. Get TAO, a subnet&rsquo;s alpha token or
          service credits. Every token is backed on Bittensor, and sells back for {STABLE}.
        </p>
      </header>

      {!rails && (
        <Panel label='Not live yet'>
          <p className={styles.note}>
            The rails are not on {railsConfig.spoke.name} yet. This page turns on when the portal is
            deployed there.
          </p>
        </Panel>
      )}

      {rails && (
        <Panel label='1 · Wallet' lit={connected}>
          {connected && wallet.account ? (
            <>
              <Row label={wallet.local ? 'Local test account' : 'Account'}>
                <span title={wallet.account}>{short(wallet.account)}</span>
              </Row>
              <Row label={STABLE}>
                {portfolio.stable === null
                  ? '…'
                  : formatAmount(portfolio.stable, STABLE_DECIMALS, 2)}
              </Row>
              {railsConfig.local.faucet && (
                <button className={styles.ghost} onClick={mint} disabled={busy !== null}>
                  Get 1,000 test {STABLE}
                </button>
              )}
            </>
          ) : (
            <>
              <button
                className={styles.primary}
                onClick={wallet.connect}
                disabled={wallet.connecting}
              >
                {wallet.connecting ? 'Connecting…' : 'Connect wallet'}
              </button>
              {railsConfig.local.testKey && (
                <button className={styles.ghost} onClick={wallet.connectLocal}>
                  Use the local test account
                </button>
              )}
              <p className={styles.note}>
                {wallet.available
                  ? `Your wallet switches to ${railsConfig.spoke.name}.`
                  : 'You need a wallet such as MetaMask.'}
              </p>
            </>
          )}
          {wallet.error && <p className={styles.problem}>{wallet.error}</p>}
        </Panel>
      )}

      {rails && connected && (
        <Panel label='2 · Choose' lit={Boolean(mode === 'credits' ? service : token)}>
          <Segmented
            value={mode}
            onChange={(next) => {
              setMode(next);
              setAmountText('');
            }}
            options={[
              {value: 'token', label: 'Subnet token'},
              {value: 'tao', label: 'TAO'},
              {value: 'credits', label: 'Credits'},
            ]}
          />
          {mode === 'token' &&
            (tokens === null ? (
              <p className={styles.note}>Loading the tokens…</p>
            ) : alphaTokens.length === 0 ? (
              <p className={styles.note}>No subnet has a token on this chain yet.</p>
            ) : (
              <label className={styles.field}>
                <span className={styles.label}>Subnet</span>
                <select
                  className={styles.select}
                  value={netuid ?? ''}
                  onChange={(event) => {
                    setNetuid(Number(event.target.value));
                    setAmountText('');
                  }}
                >
                  {alphaTokens.map((option) => (
                    <option key={option.netuid} value={option.netuid}>
                      {option.symbol} · {option.name} · subnet {option.netuid}
                    </option>
                  ))}
                </select>
              </label>
            ))}
          {mode === 'tao' && (
            <p className={styles.note}>
              One TAO here is one TAO held in reserve on Bittensor. It is the way into every subnet.
            </p>
          )}
          {mode === 'credits' &&
            (services.length === 0 ? (
              <p className={styles.note}>No service takes credits here yet.</p>
            ) : (
              <label className={styles.field}>
                <span className={styles.label}>Service</span>
                <select
                  className={styles.select}
                  value={serviceAccount ?? ''}
                  onChange={(event) => {
                    setServiceAccount(event.target.value);
                    setAmountText('');
                  }}
                >
                  {services.map((option) => (
                    <option key={option.account} value={option.account}>
                      {option.name} credits · subnet {option.netuid}
                    </option>
                  ))}
                </select>
              </label>
            ))}
        </Panel>
      )}

      {rails && connected && (mode === 'credits' ? service : token) && (
        <Panel
          label={`3 · ${mode === 'credits' ? 'Buy credits' : 'Trade'}`}
          lit={quote.state === 'ready' && !exceeds}
        >
          <Holdings
            mode={mode}
            token={token}
            holding={holding}
            service={service}
            credits={service ? (portfolio.credits[service.account] ?? null) : null}
          />
          <hr className={styles.rule} />
          {mode !== 'credits' && (
            <Segmented
              value={side}
              onChange={(next) => {
                setSide(next);
                setAmountText('');
              }}
              options={[
                {value: 'buy', label: 'Buy'},
                {value: 'sell', label: 'Sell'},
              ]}
            />
          )}
          <label className={styles.field}>
            <span className={styles.label}>
              You pay
              {available !== null && (
                <button type='button' className={styles.max} onClick={useMax}>
                  Max {formatAmount(available, effectiveSide === 'buy' ? STABLE_DECIMALS : 9, 4)}
                </button>
              )}
            </span>
            <span className={styles.amount}>
              <input
                className={styles.input}
                inputMode='decimal'
                placeholder='0.00'
                value={amountText}
                onChange={(event) => setAmountText(event.target.value)}
                aria-invalid={exceeds}
              />
              <span className={styles.unit}>
                {effectiveSide === 'buy' ? STABLE : (token?.symbol ?? '')}
              </span>
            </span>
          </label>
          <QuoteBox trade={trade} quote={quote} exceeds={exceeds} amountText={amountText} />
          <button
            className={styles.primary}
            onClick={submit}
            disabled={busy !== null || !trade || exceeds || quote.state !== 'ready'}
          >
            {busy ?? actionLabel(mode, effectiveSide, token)}
          </button>
          {mode === 'credits' ? (
            <p className={styles.note}>
              One credit is one dollar of the subnet&rsquo;s alpha, burned at the service. Credits
              are spent when you buy them: they belong to your address at this service, and cannot
              be sold or refunded afterwards.
            </p>
          ) : (
            token &&
            !wallet.local && (
              <button
                className={styles.ghost}
                onClick={() => wallet.watchToken(token.address, token.symbol, 9).catch(() => {})}
              >
                Watch {token.symbol} in your wallet
              </button>
            )
          )}
        </Panel>
      )}

      {order.progress && <Tracker progress={order.progress} onClose={order.clear} />}

      {log.length > 0 && (
        <ol className={styles.log} aria-live='polite'>
          {log.map((line) => (
            <li key={`${line.at}-${line.text}`} className={styles[line.tone]}>
              <time>{new Date(line.at).toLocaleTimeString()}</time>
              {line.text}
            </li>
          ))}
        </ol>
      )}
    </div>
  );
}

/** Reads the form into a trade, or `null` while it does not describe one. */
function describeTrade(
  mode: Mode,
  side: Side,
  token: Token | null,
  service: Service | null,
  account: string | null,
  amountText: string,
): Trade | null {
  if (!account) return null;
  const paysStable = side === 'buy';
  const amount = parseAmount(amountText, paysStable ? STABLE_DECIMALS : 9);
  if (amount === null || amount === ZERO) return null;
  const amountHub = paysStable ? toHubUnits(amount) : amount;

  if (mode === 'credits') {
    if (!service) return null;
    return {
      source: {kind: 'stable'},
      target: {
        kind: 'credits',
        netuid: service.netuid,
        service: service.account,
        beneficiary: account,
      },
      amountHub,
      amountWallet: amount,
      payUnit: STABLE,
      receiveUnit: 'credits',
      receiveDigits: 2,
      route: `${STABLE} → tUSD → TAO → alpha → credits`,
    };
  }
  if (!token) return null;
  const isTao = token.kind === 'tao';
  const asset: Source = isTao ? {kind: 'tao'} : {kind: 'alpha', netuid: token.netuid};
  if (paysStable) {
    return {
      source: {kind: 'stable'},
      target: isTao
        ? {kind: 'tao', to: account}
        : {kind: 'alpha', netuid: token.netuid, to: account},
      amountHub,
      amountWallet: amount,
      payUnit: STABLE,
      receiveUnit: token.symbol,
      receiveDigits: 4,
      route: isTao ? `${STABLE} → tUSD → TAO` : `${STABLE} → tUSD → TAO → alpha`,
    };
  }
  return {
    source: asset,
    target: {kind: 'stable', to: account},
    amountHub,
    amountWallet: amount,
    payUnit: token.symbol,
    receiveUnit: STABLE,
    receiveDigits: 2,
    route: isTao ? `TAO → tUSD → ${STABLE}` : `alpha → TAO → tUSD → ${STABLE}`,
  };
}

function actionLabel(mode: Mode, side: Side, token: Token | null): string {
  switch (mode) {
    case 'credits':
      return 'Buy credits';
    case 'tao':
    case 'token':
      return `${side === 'buy' ? 'Buy' : 'Sell'} ${token?.symbol ?? ''}`.trim();
    default: {
      const unknown: never = mode;
      return unknown;
    }
  }
}

function Panel({label, lit, children}: {label: string; lit?: boolean; children: ReactNode}) {
  return (
    <section className={styles.panel}>
      <div className={styles.bar}>
        <span>{label}</span>
        <span className={clsx(styles.dot, lit && styles.dotLit)} aria-hidden='true' />
      </div>
      <div className={styles.body}>{children}</div>
    </section>
  );
}

function Row({label, children}: {label: string; children: ReactNode}) {
  return (
    <p className={styles.row}>
      <span className={styles.label}>{label}</span>
      <span className={styles.value}>{children}</span>
    </p>
  );
}

function Segmented<T extends string>({
  value,
  options,
  onChange,
}: {
  value: T;
  options: {value: T; label: string}[];
  onChange: (value: T) => void;
}) {
  return (
    <div className={styles.segmented} role='tablist'>
      {options.map((option) => (
        <button
          key={option.value}
          type='button'
          role='tab'
          aria-selected={option.value === value}
          className={clsx(styles.segment, option.value === value && styles.segmentOn)}
          onClick={() => onChange(option.value)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

function Holdings({
  mode,
  token,
  holding,
  service,
  credits,
}: {
  mode: Mode;
  token: Token | null;
  holding: Holding | null;
  service: Service | null;
  credits: bigint | null;
}) {
  if (mode === 'credits') {
    return (
      <div className={styles.holding}>
        <p className={styles.big}>
          {credits === null ? '…' : formatUnits9(credits, 2)}
          <span className={styles.bigUnit}>credits</span>
        </p>
        <p className={styles.sub}>spent at {service?.name ?? 'this service'} for your address</p>
      </div>
    );
  }
  if (!token) return null;
  const change =
    holding?.worthAlpha != null && holding.principalAlpha != null && holding.balance > ZERO
      ? percentChange(holding.principalAlpha, holding.worthAlpha)
      : null;
  return (
    <div className={styles.holding}>
      <p className={styles.big}>
        {holding ? formatUnits9(holding.balance) : '…'}
        <span className={styles.bigUnit}>{token.symbol}</span>
      </p>
      {token.kind === 'tao' ? (
        <p className={styles.sub}>in your wallet, each backed by one TAO on Bittensor</p>
      ) : (
        <p className={styles.sub}>
          {holding?.worthAlpha != null
            ? `worth ${formatUnits9(holding.worthAlpha)} alpha`
            : 'in your wallet'}
          {change && (
            <span className={clsx(styles.change, change.startsWith('−') && styles.changeDown)}>
              {change} since bought
            </span>
          )}
        </p>
      )}
    </div>
  );
}

function QuoteBox({
  trade,
  quote,
  exceeds,
  amountText,
}: {
  trade: Trade | null;
  quote: QuoteState;
  exceeds: boolean;
  amountText: string;
}) {
  if (!trade) {
    return (
      <p className={styles.quoteIdle}>
        {amountText.trim() === '' ? 'Enter an amount to see what you receive.' : 'Not an amount.'}
      </p>
    );
  }
  if (exceeds) return <p className={styles.problem}>That is more than you hold.</p>;
  switch (quote.state) {
    case 'idle':
    case 'loading':
      return <p className={styles.quoteIdle}>Asking Bittensor for a price…</p>;
    case 'refused':
      return <p className={styles.problem}>{explainRefusal(quote.reason)}</p>;
    case 'error':
      return <p className={styles.problem}>No price: {quote.message}</p>;
    case 'ready': {
      // The hub answers in 9 decimals, whatever the asset.
      const receive = formatUnits9(quote.amountOut, trade.receiveDigits);
      const minimum = formatUnits9(
        lessBps(quote.amountOut, DEFAULT_SLIPPAGE_BPS),
        trade.receiveDigits,
      );
      return (
        <div className={styles.quote}>
          <Row label='You receive'>
            about {receive} {trade.receiveUnit}
          </Row>
          <Row label='At least'>
            {minimum} {trade.receiveUnit}
          </Row>
          <Row label='Route'>{trade.route}</Row>
          <p className={styles.note}>
            If the price moves past the minimum, the order is undone and you get back exactly what
            you paid.
          </p>
        </div>
      );
    }
    default: {
      const unknown: never = quote;
      return unknown;
    }
  }
}

type StepState = 'done' | 'active' | 'waiting' | 'failed';

/** The mark before a step: a tick, a cross, or a dot. Drawn, so that no font is needed. */
function StepMark({state}: {state: StepState}) {
  const stroke = {
    fill: 'none',
    stroke: 'currentColor',
    strokeWidth: 1.6,
    strokeLinecap: 'square' as const,
  };
  return (
    <svg className={styles.glyph} viewBox='0 0 12 12' aria-hidden='true'>
      {state === 'done' && <path d='M2 6.5 5 9.5 10 3' {...stroke} />}
      {state === 'failed' && <path d='M3 3 9 9M9 3 3 9' {...stroke} />}
      {(state === 'active' || state === 'waiting') && (
        <circle cx='6' cy='6' r='2.5' fill='currentColor' />
      )}
    </svg>
  );
}

function Tracker({progress, onClose}: {progress: OrderProgress; onClose: () => void}) {
  const elapsed = useElapsed(progress.startedAt, progress.finishedAt);
  const {status, answer, paysOut} = progress;
  const hubDone = status !== 'pending';
  const refused = status === 'refunded' || status === 'rejected' || status === 'skipped';
  const paid = answer !== null;

  const hubLabel = (() => {
    switch (status) {
      case 'pending':
        return 'Bittensor settles the order';
      case 'settled':
        return 'Settled on Bittensor';
      case 'refunded':
        return 'Bittensor could not settle it: a refund is on its way';
      case 'rejected':
      case 'skipped':
        return 'Bittensor did not take the order';
      default: {
        const unknown: never = status;
        return unknown;
      }
    }
  })();

  const steps: {label: string; state: StepState; detail?: string}[] = [
    {label: 'Sent from your wallet', state: 'done', detail: `order ${progress.sent.nonce}`},
    {
      label: hubLabel,
      state: !hubDone ? 'active' : refused ? 'failed' : 'done',
      detail:
        status === 'settled' && progress.amountOut !== null && !paysOut
          ? `${formatUnits9(progress.amountOut, 2)} credits`
          : undefined,
    },
  ];
  if (paysOut || status === 'refunded') {
    steps.push({
      label: answer?.refund
        ? 'Refunded to your wallet'
        : paid
          ? 'Paid to your wallet'
          : status === 'refunded'
            ? 'The refund arrives'
            : 'The payment arrives',
      state: paid ? 'done' : hubDone ? 'active' : 'waiting',
      detail: answer && !answer.refund ? undefined : answer ? 'exactly what you paid' : undefined,
    });
  }

  return (
    <section className={styles.panel} aria-live='polite'>
      <div className={styles.bar}>
        <span>Order</span>
        <span className={styles.value}>
          {elapsed}s
          {progress.finishedAt !== null && (
            <button type='button' className={styles.close} onClick={onClose}>
              Close
            </button>
          )}
        </span>
      </div>
      <div className={styles.body}>
        <ol className={styles.steps}>
          {steps.map((step) => (
            <li key={step.label} className={clsx(styles.step, styles[step.state])}>
              <StepMark state={step.state} />
              <span className={styles.stepLabel}>{step.label}</span>
              {step.detail && <span className={styles.stepDetail}>{step.detail}</span>}
            </li>
          ))}
        </ol>
        {progress.finishedAt === null && (
          <p className={styles.note}>
            Messages between the chains take a minute or two. You can leave this page: the order
            completes on its own.
          </p>
        )}
      </div>
    </section>
  );
}
