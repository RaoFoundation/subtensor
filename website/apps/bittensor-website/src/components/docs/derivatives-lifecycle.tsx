'use client';

import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { ExplainerPanel, ExplainerToggle } from './explainer-panel';
import { ACCENT, ACCENT_WASH, INK, INK_FAINT } from './chart-theme';
import { CUSHION, LEVERAGE, OPEN_PRICE, feePerDay, lift, phi, simulate, type Outcome as Numbers, type Side } from '@/lib/derivatives-math';

type Outcome = 'down' | 'up';

// The shared worked example, closed after 7 days with a 20% move either way.
const DAYS = 7;
const WALLET = 1_000;
const MOVE_PCT = 20;

const tao = (v: number, d = 2) => `${v.toLocaleString('en-US', { minimumFractionDigits: d, maximumFractionDigits: d })} τ`;
const alpha = (v: number) => `${v.toLocaleString('en-US', { maximumFractionDigits: 0 })} α`;
const price = (v: number) => `${v.toFixed(4)} τ/α`;
const signed = (v: number) => `${v >= 0 ? '+' : '−'}${tao(Math.abs(v))}`;

const FONT = { fontFamily: 'FiraCode, monospace', fontSize: 10.5, letterSpacing: 0.4 } as const;
const MOVE_TRANSITION = 'transform 700ms cubic-bezier(0.4, 0, 0.2, 1), opacity 450ms ease';

// Scene geometry (viewBox 760 × 300).
const YOU = { x: 14, y: 118, w: 156, h: 92 };
const POS = { x: 556, y: 104, w: 184, h: 120 };
const BAR_W = 42;
const BAR_TOP = 116;
const BAR_H = 148;
const SLICE_H = 22;
const TRAY_Y = 56;
const LEFT_BAR_X = 292;
const RIGHT_BAR_X = 352;
const BARS_MID = (LEFT_BAR_X + RIGHT_BAR_X + BAR_W) / 2;

/** The eight slides, in order. Each is one scene of the same picture. */
const PHASES = ['deposit', 'lift', 'trade', 'open', 'move', 'reverse', 'return', 'payout'] as const;
type Phase = (typeof PHASES)[number];
const STEP_COUNT = PHASES.length;

interface Slide {
  title: string;
  body: string;
}

function slide(phase: Phase, side: Side, outcome: Outcome, n: Numbers): Slide {
  const short = side === 'short';
  const fell = outcome === 'down';
  const win = short === fell;
  const { tao: LIFT_TAO, alpha: LIFT_ALPHA } = lift(side);
  const sharePct = `${phi(side) * 100}%`;
  const lev = `${LEVERAGE[side]}x`;
  switch (phase) {
    case 'deposit':
      return {
        title: 'You put down a cushion',
        body: `You send ${tao(CUSHION, 0)} to the pallet. This is your deposit: the most you can lose. Nothing else leaves your wallet.`,
      };
    case 'lift':
      return {
        title: 'The pool lends you a slice',
        body: `At ${lev} your ${tao(CUSHION, 0)} sizes a ${sharePct} slice. The pallet lifts ${sharePct} of both reserves — ${tao(LIFT_TAO, 0)} and ${alpha(LIFT_ALPHA)} — out of the pool. Both sides shrink by the same share, so the price stays at ${price(OPEN_PRICE)}.`,
      };
    case 'trade':
      return {
        title: short ? 'Half the slice is sold' : 'Half the slice is spent',
        body: short
          ? `The ${alpha(LIFT_ALPHA)} is sold straight back into the pool for ${tao(n.proceeds)}. It is a real swap, so the price dips to ${price(n.priceOpen)}. The ${tao(LIFT_TAO, 0)} half waits as escrow.`
          : `The ${tao(LIFT_TAO, 0)} buys ${alpha(n.proceeds)} straight from the pool. It is a real swap, so the price rises to ${price(n.priceOpen)}. The ${alpha(LIFT_ALPHA)} half waits as escrow.`,
      };
    case 'open':
      return {
        title: 'Your position is open',
        body: short
          ? `It holds ${tao(CUSHION + n.proceeds)} (cushion + proceeds) and owes ${alpha(LIFT_ALPHA)} to the pool. A 30-day clock starts; the fee is ${tao(feePerDay('short'))} per day (6 τ × the ${sharePct} of the pool lifted, scaled a little for slippage).`
          : `It holds your ${tao(CUSHION, 0)} cushion plus ${alpha(n.proceeds)}, and owes ${tao(LIFT_TAO, 0)} to the pool. A 30-day clock starts; the fee is ${tao(feePerDay('long'))} per day (0.01% of ${tao(LIFT_TAO, 0)}, scaled a little for slippage).`,
      };
    case 'move':
      return {
        title: `Alpha ${fell ? 'falls' : 'rises'} ${MOVE_PCT}%`,
        body: short
          ? `Buying ${alpha(LIFT_ALPHA)} back would now cost ${tao(n.closeLeg)} instead of ${tao(n.proceeds)}. You are ${win ? 'up' : 'down'} about ${tao(Math.abs(n.closeLeg - n.proceeds), 0)}. ${DAYS} days pass: ${tao(n.fee)} of fee has accrued.`
          : `Selling ${alpha(n.proceeds)} would now raise ${tao(n.closeLeg)} instead of ${tao(LIFT_TAO, 0)}. You are ${win ? 'up' : 'down'} about ${tao(Math.abs(n.closeLeg - LIFT_TAO), 0)}. ${DAYS} days pass: ${tao(n.fee)} of fee has accrued.`,
      };
    case 'reverse':
      return {
        title: 'You close: the trade is reversed',
        body: short
          ? `The pallet spends ${tao(n.closeLeg)} of the position's TAO to buy ${alpha(LIFT_ALPHA)} back from the pool.`
          : `The pallet sells the ${alpha(n.proceeds)} back to the pool for ${tao(n.closeLeg)}, then repays the ${tao(LIFT_TAO, 0)} it borrowed.`,
      };
    case 'return':
      return {
        title: 'The slice goes home, plus the fee',
        body: short
          ? `${alpha(LIFT_ALPHA)} and the ${tao(LIFT_TAO, 0)} escrow return to the pool together with the ${tao(n.fee)} fee. Uneven amounts are added without moving the price.`
          : `${tao(LIFT_TAO, 0)} and the ${alpha(LIFT_ALPHA)} escrow return to the pool together with the ${tao(n.fee)} fee. Uneven amounts are added without moving the price.`,
      };
    case 'payout':
      return {
        title: win ? 'You take the profit' : 'You take the loss',
        body: `${tao(n.payout)} comes back to your wallet: your ${tao(CUSHION, 0)} cushion ${n.pnl >= 0 ? 'plus' : 'minus'} ${tao(Math.abs(n.pnl))}. Nothing was minted, nothing was burned — the pool is ${n.pnl >= 0 ? 'lighter' : 'heavier'} by the same amount.`,
      };
    default: {
      const exhaustive: never = phase;
      return exhaustive;
    }
  }
}

/** Where a lifted slice is drawn: sitting in its bar (nothing to draw), lifted into the tray, or filled back home. */
type SliceState = 'bar' | 'tray' | 'home';

interface PanelLine {
  k: string;
  v: string;
  accent?: boolean;
}

/** Everything the picture needs for one slide. Built by `scene`, read by the SVG. */
interface Scene {
  traded: SliceState;
  escrow: SliceState;
  trayLabel: string | null;
  price: number;
  priceMoved: boolean;
  cushion: 'wallet' | 'position' | 'back';
  /** The proceeds coin: where it sits, and whether it is drawn (it stays parked while hidden so the next move starts from the right place). */
  proceeds: { at: 'bar' | 'position' | 'closing'; visible: boolean };
  position: PanelLine[];
  positionOpen: boolean;
  clock: 'hidden' | 'idle' | 'running';
  pnlBadge: boolean;
  /** The one annotation this slide adds on top of the shared picture. */
  callout: 'deposit' | 'sold' | 'fee' | 'return' | null;
  wallet: number;
}

function scene(phase: Phase, side: Side, n: Numbers): Scene {
  const short = side === 'short';
  const { tao: LIFT_TAO, alpha: LIFT_ALPHA } = lift(side);
  const holds = short ? tao(n.proceeds) : alpha(n.proceeds);
  const owes = short ? alpha(LIFT_ALPHA) : tao(LIFT_TAO, 0);
  const netBeforeFee = CUSHION + (short ? n.proceeds - n.closeLeg : n.closeLeg - LIFT_TAO);
  const openLines = (fee: number): PanelLine[] => [
    { k: 'cushion', v: tao(CUSHION, 0) },
    { k: 'holds', v: holds },
    { k: 'owes', v: owes, accent: true },
    { k: 'fee so far', v: tao(fee) },
  ];
  const closingLines = (feePaid: boolean): PanelLine[] => [
    { k: 'trade reversed', v: '✓' },
    { k: 'owes', v: '—' },
    { k: 'net before fee', v: tao(netBeforeFee) },
    { k: feePaid ? 'fee paid' : 'fee due', v: tao(n.fee), accent: !feePaid },
  ];

  const start: Scene = {
    traded: 'bar',
    escrow: 'bar',
    trayLabel: null,
    price: OPEN_PRICE,
    priceMoved: false,
    cushion: 'wallet',
    proceeds: { at: 'bar', visible: false },
    position: [{ k: 'status', v: 'none yet' }],
    positionOpen: false,
    clock: 'hidden',
    pnlBadge: false,
    callout: 'deposit',
    wallet: WALLET,
  };

  switch (phase) {
    case 'deposit':
      return start;
    case 'lift':
      return {
        ...start,
        traded: 'tray',
        escrow: 'tray',
        trayLabel: `${phi(side) * 100}% LIFTED OUT`,
        cushion: 'position',
        position: [{ k: 'cushion', v: tao(CUSHION, 0) }, { k: 'status', v: 'opening…' }],
        positionOpen: true,
        callout: null,
        wallet: WALLET - CUSHION,
      };
    case 'trade':
      return {
        ...scene('lift', side, n),
        traded: 'bar',
        trayLabel: 'ESCROW WAITS HERE',
        price: n.priceOpen,
        proceeds: { at: 'position', visible: true },
        callout: 'sold',
      };
    case 'open':
      return {
        ...scene('trade', side, n),
        position: openLines(simulate(side, 0, 1).fee),
        clock: 'idle',
        callout: null,
      };
    case 'move':
      return {
        ...scene('open', side, n),
        price: n.priceClose,
        priceMoved: true,
        proceeds: { at: 'position', visible: false },
        position: openLines(n.fee),
        clock: 'running',
        pnlBadge: true,
        callout: 'fee',
      };
    case 'reverse':
      return {
        ...scene('move', side, n),
        traded: 'tray',
        trayLabel: 'SLICE READY TO GO HOME',
        proceeds: { at: 'closing', visible: true },
        position: closingLines(false),
        callout: null,
      };
    case 'return':
      return {
        ...scene('reverse', side, n),
        traded: 'home',
        escrow: 'home',
        trayLabel: null,
        proceeds: { at: 'closing', visible: false },
        position: closingLines(true),
        callout: 'return',
      };
    case 'payout':
      return {
        ...scene('return', side, n),
        cushion: 'back',
        position: [{ k: 'status', v: 'closed' }],
        clock: 'hidden',
        pnlBadge: false,
        callout: null,
        wallet: WALLET - CUSHION + n.payout,
      };
    default: {
      const exhaustive: never = phase;
      return exhaustive;
    }
  }
}

function Moving({ x, y, visible = true, children }: { x: number; y: number; visible?: boolean; children: ReactNode }) {
  return (
    <g style={{ transform: `translate(${x}px, ${y}px)`, opacity: visible ? 1 : 0, transition: MOVE_TRANSITION }}>
      {children}
    </g>
  );
}

function Coin({ label, accent = false, w = 64, fontSize = FONT.fontSize }: { label: string; accent?: boolean; w?: number; fontSize?: number }) {
  return (
    <g>
      <rect x={-w / 2} y={-11} width={w} height={22} rx={11} fill={accent ? ACCENT_WASH : 'var(--bt-bg, #fff)'} stroke={accent ? ACCENT : INK} strokeWidth={1.2} />
      <text {...FONT} fontSize={fontSize} x={0} y={3.8} textAnchor="middle" fill={accent ? ACCENT : INK} fontWeight={600}>
        {label}
      </text>
    </g>
  );
}

function Slice({ label, filled = false }: { label: string; filled?: boolean }) {
  return (
    <g>
      <rect x={0} y={0} width={BAR_W} height={SLICE_H} fill={filled ? ACCENT_WASH : 'var(--bt-bg, #fff)'} stroke={ACCENT} strokeWidth={1.2} />
      <text {...FONT} x={BAR_W / 2} y={SLICE_H / 2 + 3.5} textAnchor="middle" fill={ACCENT} fontSize={9} letterSpacing={0}>
        {label}
      </text>
    </g>
  );
}

function Panel({ x, y, w, h, title, lines, dashed = false }: { x: number; y: number; w: number; h: number; title: string; lines: { k: string; v: string; accent?: boolean }[]; dashed?: boolean }) {
  return (
    <g>
      <rect x={x} y={y} width={w} height={h} fill="var(--bt-bg, #fff)" stroke={dashed ? INK_FAINT : INK} strokeWidth={1} strokeDasharray={dashed ? '4 3' : undefined} />
      <text {...FONT} x={x + 12} y={y + 18} fill={INK} fontWeight={600}>
        {title}
      </text>
      {lines.map((line, i) => (
        <g key={line.k}>
          <text {...FONT} x={x + 12} y={y + 40 + i * 17} fill={INK_FAINT} fontSize={10}>
            {line.k}
          </text>
          <text {...FONT} x={x + w - 12} y={y + 40 + i * 17} textAnchor="end" fill={line.accent ? ACCENT : INK} fontSize={10} style={{ transition: 'fill 400ms' }}>
            {line.v}
          </text>
        </g>
      ))}
    </g>
  );
}

function Clock({ x, y, fraction, visible }: { x: number; y: number; fraction: number; visible: boolean }) {
  const r = 13;
  const a = fraction * Math.PI * 2;
  const ex = x + r * Math.sin(a);
  const ey = y - r * Math.cos(a);
  const large = a > Math.PI ? 1 : 0;
  return (
    <g style={{ opacity: visible ? 1 : 0, transition: 'opacity 450ms ease' }}>
      <circle cx={x} cy={y} r={r} fill="none" stroke={INK_FAINT} strokeWidth={1} />
      {fraction > 0 && <path d={`M ${x} ${y} L ${x} ${y - r} A ${r} ${r} 0 ${large} 1 ${ex} ${ey} Z`} fill={ACCENT_WASH} stroke={ACCENT} strokeWidth={1} />}
      <text {...FONT} x={x - r - 8} y={y + 3.5} textAnchor="end" fill={INK_FAINT} fontSize={9}>
        {fraction > 0 ? `DAY ${Math.round(fraction * 30)} / 30` : '30-DAY CLOCK'}
      </text>
    </g>
  );
}

export function DerivativesLifecycle() {
  const [side, setSide] = useState<Side>('short');
  const [outcome, setOutcome] = useState<Outcome>('down');
  const [step, setStep] = useState(0);
  const [playing, setPlaying] = useState(false);

  const phase = PHASES[step] ?? PHASES[0];
  const n = useMemo(() => simulate(side, outcome === 'down' ? -MOVE_PCT : MOVE_PCT, DAYS), [side, outcome]);
  const text = useMemo(() => slide(phase, side, outcome, n), [phase, side, outcome, n]);
  const s = useMemo(() => scene(phase, side, n), [phase, side, n]);

  useEffect(() => {
    if (!playing) return;
    if (step >= STEP_COUNT - 1) {
      setPlaying(false);
      return;
    }
    const id = setTimeout(() => setStep((v) => Math.min(STEP_COUNT - 1, v + 1)), 3600);
    return () => clearTimeout(id);
  }, [playing, step]);

  const short = side === 'short';
  const { tao: LIFT_TAO, alpha: LIFT_ALPHA } = lift(side);

  // Which bar is which asset: the traded asset sits on the left, the escrow asset on the right.
  const tradedLabel = short ? 'α' : 'τ';
  const escrowLabel = short ? 'τ' : 'α';
  const tradedSliceLabel = short ? alpha(LIFT_ALPHA) : tao(LIFT_TAO, 0);
  const escrowSliceLabel = short ? tao(LIFT_TAO, 0) : alpha(LIFT_ALPHA);
  const sliceY = (state: SliceState) => (state === 'tray' ? TRAY_Y + 4 : BAR_TOP);

  const walletSpot = { x: YOU.x + YOU.w / 2, y: YOU.y + 62 };
  const cushionAt = s.cushion === 'position' ? { x: POS.x + POS.w / 2, y: POS.y + POS.h + 18 } : walletSpot;
  const cushionLabel = s.cushion === 'back' ? tao(n.payout) : tao(CUSHION, 0);

  // Proceeds coin: emerges from the escrow-asset bar, sits above the position, then heads back to the pool on close.
  const proceedsLabel = short ? tao(n.proceeds) : alpha(n.proceeds);
  const gapMid = (RIGHT_BAR_X + BAR_W + POS.x) / 2;
  const proceedsAt = {
    bar: { x: RIGHT_BAR_X + BAR_W / 2, y: BAR_TOP + 70 },
    position: { x: POS.x + POS.w / 2, y: POS.y - 18 },
    closing: { x: gapMid + 14, y: BAR_TOP + 40 },
  }[s.proceeds.at];

  const controlClass =
    'px-2.5 py-1 font-mono text-[0.6875rem] uppercase tracking-[0.08em] text-mute transition-colors hover:bg-panel disabled:opacity-30 disabled:hover:bg-bg';

  const reset = () => {
    setStep(0);
    setPlaying(false);
  };

  return (
    <ExplainerPanel
      title="A position, start to finish"
      tag={`slide ${step + 1} / ${STEP_COUNT}`}
      caption="One scene, eight slides. Your wallet on the left, the subnet pool in the middle, your position on the right. Press play, or step through."
    >
      <div className="mb-5 flex flex-wrap gap-x-8 gap-y-3">
        <ExplainerToggle
          label="side"
          options={[
            { id: 'short', label: 'short' },
            { id: 'long', label: 'long' },
          ]}
          value={side}
          onChange={(id) => {
            setSide(id);
            reset();
          }}
        />
        <ExplainerToggle
          label="what happens next"
          options={[
            { id: 'down', label: 'alpha falls 20%' },
            { id: 'up', label: 'alpha rises 20%' },
          ]}
          value={outcome}
          onChange={setOutcome}
        />
      </div>

      <div className="overflow-x-auto">
        <svg
          viewBox="0 0 760 300"
          className="w-full min-w-[640px]"
          role="img"
          aria-label={`${text.title}. ${text.body}`}
        >
          <defs>
            <marker id="deriv-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
              <path d="M 0 0 L 10 5 L 0 10 z" fill={INK} />
            </marker>
          </defs>

          {/* Wallet */}
          <Panel
            x={YOU.x}
            y={YOU.y}
            w={YOU.w}
            h={YOU.h}
            title="YOUR WALLET"
            lines={[{ k: 'balance', v: tao(s.wallet) }]}
          />

          {/* Pool */}
          <text {...FONT} x={BARS_MID} y={TRAY_Y - 26} textAnchor="middle" fill={INK} fontWeight={600}>
            SUBNET POOL
          </text>
          <text {...FONT} x={BARS_MID} y={TRAY_Y - 12} textAnchor="middle" fill={s.priceMoved ? ACCENT : INK_FAINT} fontSize={9.5} style={{ transition: 'fill 400ms' }}>
            {price(s.price).toUpperCase()}
            {s.priceMoved ? (outcome === 'down' ? ' ↓' : ' ↑') : ''}
          </text>
          {[LEFT_BAR_X, RIGHT_BAR_X].map((x, i) => (
            <g key={x}>
              <rect x={x} y={BAR_TOP} width={BAR_W} height={BAR_H} fill="rgba(41,41,41,0.06)" stroke={INK} strokeWidth={1} />
              <text {...FONT} x={x + BAR_W / 2} y={BAR_TOP + BAR_H + 16} textAnchor="middle" fill={INK}>
                {i === 0 ? tradedLabel : escrowLabel}
              </text>
            </g>
          ))}
          {/* Holes left when a slice is lifted */}
          <rect x={LEFT_BAR_X} y={BAR_TOP} width={BAR_W} height={SLICE_H} fill="var(--bt-bg, #fff)" stroke={INK_FAINT} strokeWidth={1} strokeDasharray="3 2" style={{ opacity: s.traded === 'tray' ? 1 : 0, transition: 'opacity 450ms ease' }} />
          <rect x={RIGHT_BAR_X} y={BAR_TOP} width={BAR_W} height={SLICE_H} fill="var(--bt-bg, #fff)" stroke={INK_FAINT} strokeWidth={1} strokeDasharray="3 2" style={{ opacity: s.escrow === 'tray' ? 1 : 0, transition: 'opacity 450ms ease' }} />

          {/* Tray label */}
          <text {...FONT} x={BARS_MID} y={TRAY_Y - 0} textAnchor="middle" fill={ACCENT} fontSize={9} style={{ opacity: s.trayLabel ? 1 : 0, transition: 'opacity 450ms ease' }}>
            {s.trayLabel ?? ''}
          </text>

          {/* Slices */}
          <Moving x={LEFT_BAR_X} y={sliceY(s.traded)} visible={s.traded !== 'bar'}>
            <Slice label={tradedSliceLabel} filled={s.traded === 'home'} />
          </Moving>
          <Moving x={RIGHT_BAR_X} y={sliceY(s.escrow)} visible={s.escrow !== 'bar'}>
            <Slice label={s.escrow === 'home' ? escrowSliceLabel : 'ESCROW'} filled={s.escrow === 'home'} />
          </Moving>

          {/* Sold slice sinks into the bar */}
          <g style={{ opacity: s.callout === 'sold' ? 1 : 0, transition: 'opacity 450ms ease' }}>
            <path d={`M ${LEFT_BAR_X + BAR_W / 2} ${TRAY_Y + SLICE_H + 8} L ${LEFT_BAR_X + BAR_W / 2} ${BAR_TOP - 4}`} stroke={INK} strokeWidth={1.2} fill="none" markerEnd="url(#deriv-arrow)" />
            <text {...FONT} x={LEFT_BAR_X - 6} y={(TRAY_Y + BAR_TOP) / 2 + 10} textAnchor="end" fill={INK} fontSize={9.5}>
              {short ? 'SOLD' : 'SPENT'}
            </text>
          </g>

          {/* Fee drip */}
          <g style={{ opacity: s.callout === 'fee' ? 1 : 0, transition: 'opacity 450ms ease' }}>
            {[0, 1, 2, 3, 4].map((i) => (
              <circle key={i} cx={POS.x - 14 - i * 22} cy={BAR_TOP + BAR_H - 14 - i * 4} r={1.8} fill={INK} />
            ))}
            <text {...FONT} x={(POS.x + RIGHT_BAR_X + BAR_W) / 2} y={BAR_TOP + BAR_H + 8} textAnchor="middle" fill={INK_FAINT} fontSize={9}>
              FEE {tao(feePerDay(side))} / DAY
            </text>
          </g>

          {/* The slice and fee go home */}
          <g style={{ opacity: s.callout === 'return' ? 1 : 0, transition: 'opacity 450ms ease' }}>
            <text {...FONT} x={BARS_MID} y={TRAY_Y + 4} textAnchor="middle" fill={ACCENT} fontSize={9.5} fontWeight={600}>
              SLICE BACK + {tao(n.fee)} FEE
            </text>
          </g>

          {/* Position */}
          <Panel x={POS.x} y={POS.y} w={POS.w} h={POS.h} title={`YOUR ${short ? 'SHORT' : 'LONG'}`} lines={s.position} dashed={!s.positionOpen} />
          <Clock x={POS.x + POS.w - 16} y={POS.y - 48} fraction={s.clock === 'running' ? DAYS / 30 : 0} visible={s.clock !== 'hidden'} />

          {/* P&L badge */}
          <Moving x={POS.x + 62} y={POS.y - 18} visible={s.pnlBadge}>
            <Coin label={`${signed(n.pnl + n.fee)} · ${n.pnl + n.fee >= 0 ? 'UP' : 'DOWN'}`} accent={n.pnl + n.fee < 0} w={118} />
          </Moving>

          {/* Coins in motion */}
          <Moving x={proceedsAt.x} y={proceedsAt.y} visible={s.proceeds.visible && s.proceeds.at === 'position'}>
            <Coin label={proceedsLabel} w={78} />
          </Moving>
          <Moving x={proceedsAt.x} y={proceedsAt.y} visible={s.proceeds.visible && s.proceeds.at === 'closing'}>
            <Coin label={short ? `${tao(n.closeLeg)} → ${alpha(LIFT_ALPHA)}` : `${alpha(n.proceeds)} → ${tao(n.closeLeg)}`} w={132} fontSize={9.5} />
            <path d={`M -68 0 L ${RIGHT_BAR_X + BAR_W - proceedsAt.x + 6} 0`} stroke={INK} strokeWidth={1.2} fill="none" markerEnd="url(#deriv-arrow)" />
            <text {...FONT} x={0} y={-18} textAnchor="middle" fill={INK_FAINT} fontSize={9}>
              {short ? 'REBUY FROM POOL' : 'SELL TO POOL, REPAY'}
            </text>
          </Moving>
          <Moving x={cushionAt.x} y={cushionAt.y}>
            <Coin label={cushionLabel} accent={s.cushion === 'back' && n.pnl < 0} w={s.cushion === 'back' ? 92 : 64} />
          </Moving>
          <text {...FONT} x={cushionAt.x} y={cushionAt.y + 24} textAnchor="middle" fill={INK_FAINT} fontSize={9} style={{ transition: MOVE_TRANSITION }}>
            {s.cushion === 'back' ? (n.pnl >= 0 ? 'CUSHION + PROFIT − FEE' : 'CUSHION − LOSS − FEE') : 'CUSHION'}
          </text>

          {/* Direction arrows between wallet and position */}
          <g style={{ opacity: s.callout === 'deposit' ? 1 : 0, transition: 'opacity 450ms ease' }}>
            <path d={`M ${YOU.x + YOU.w + 8} ${YOU.y + YOU.h / 2} L ${LEFT_BAR_X - 30} ${YOU.y + YOU.h / 2}`} stroke={INK_FAINT} strokeWidth={1} strokeDasharray="4 3" fill="none" />
            <path d={`M ${RIGHT_BAR_X + BAR_W + 30} ${YOU.y + YOU.h / 2} L ${POS.x - 8} ${YOU.y + YOU.h / 2}`} stroke={INK_FAINT} strokeWidth={1} strokeDasharray="4 3" fill="none" markerEnd="url(#deriv-arrow)" />
            <text {...FONT} x={(YOU.x + YOU.w + LEFT_BAR_X) / 2 - 8} y={YOU.y + YOU.h / 2 - 8} textAnchor="middle" fill={INK_FAINT} fontSize={9}>
              TO THE PALLET
            </text>
          </g>
        </svg>
      </div>

      {/* Slide text */}
      <div className="mt-4 min-h-[5.5rem] border-t border-line pt-4">
        <p className="font-mono text-[0.8125rem] font-medium uppercase tracking-[0.06em]" style={{ color: INK }}>
          {String(step + 1).padStart(2, '0')} · {text.title}
        </p>
        <p className="mt-2 max-w-2xl text-[0.8125rem] leading-relaxed text-mute">{text.body}</p>
      </div>

      {/* Controls */}
      <div className="mt-4 flex flex-wrap items-center gap-4">
        <div className="inline-flex divide-x divide-line border border-line">
          <button type="button" onClick={() => { setPlaying(false); setStep((s) => Math.max(0, s - 1)); }} disabled={step === 0} className={controlClass}>
            ← prev
          </button>
          <button
            type="button"
            onClick={() => {
              if (step >= STEP_COUNT - 1) setStep(0);
              setPlaying((p) => !p);
            }}
            className={controlClass}
            aria-pressed={playing}
          >
            {playing ? 'pause' : step >= STEP_COUNT - 1 ? 'replay' : 'play'}
          </button>
          <button type="button" onClick={() => { setPlaying(false); setStep((s) => Math.min(STEP_COUNT - 1, s + 1)); }} disabled={step === STEP_COUNT - 1} className={controlClass}>
            next →
          </button>
        </div>
        <div className="flex items-center gap-1.5" role="tablist" aria-label="slides">
          {Array.from({ length: STEP_COUNT }, (_, i) => (
            <button
              key={i}
              type="button"
              role="tab"
              aria-selected={i === step}
              aria-label={`slide ${i + 1}`}
              onClick={() => { setPlaying(false); setStep(i); }}
              className="h-3 w-3 p-0.5"
            >
              <span className="block h-full w-full rounded-full transition-colors" style={{ backgroundColor: i === step ? INK : i < step ? INK_FAINT : 'rgba(41,41,41,0.15)' }} />
            </button>
          ))}
        </div>
      </div>

      <p className="mt-4 border-t border-line pt-3 text-[0.6875rem] leading-relaxed text-mute">
        Example pool 10,000 τ / 200,000 α at 0.05 τ/α; the slice is drawn larger than it is so you can see it.
        Shorts run at 1x and lift 1%, longs at 2x and lift 2%. Cushions are always TAO.
      </p>
    </ExplainerPanel>
  );
}
