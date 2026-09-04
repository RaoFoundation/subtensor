'use client';

import { useMemo, useState } from 'react';
import { ExplainerPanel, ExplainerToggle } from './explainer-panel';
import { ACCENT, ACCENT_REGION, INK, INK_FAINT } from './chart-theme';
import { CUSHION, payout, type Side } from '@/lib/derivatives-math';

const MOVE_MIN = -100;
const MOVE_MAX = 100;

const FONT = { fontFamily: 'FiraCode, monospace', fontSize: 9.5, letterSpacing: 0.6 } as const;

const X0 = 56;
const W = 660;
const Y0 = 28;
const H = 220;
const Y_MAX = 220;
const BASE = Y0 + H;

const xFor = (m: number) => X0 + ((m - MOVE_MIN) / (MOVE_MAX - MOVE_MIN)) * W;
const yFor = (v: number) => BASE - (Math.min(v, Y_MAX) / Y_MAX) * H;

export function DerivativesPayoff() {
  const [side, setSide] = useState<Side>('short');

  const { linePath, gainPath, lossPath, wipeout } = useMemo(() => {
    const pts: { m: number; v: number }[] = [];
    for (let m = MOVE_MIN; m <= MOVE_MAX; m += 1) pts.push({ m, v: payout(side, m) });

    const line = pts.map((p, i) => `${i === 0 ? 'M' : 'L'} ${xFor(p.m).toFixed(1)} ${yFor(p.v).toFixed(1)}`).join(' ');
    const above = pts.filter((p) => p.v >= CUSHION);
    const below = pts.filter((p) => p.v < CUSHION);
    const area = (seg: { m: number; v: number }[]) =>
      seg.length < 2
        ? ''
        : `M ${xFor(seg[0].m).toFixed(1)} ${yFor(CUSHION).toFixed(1)} ` +
          seg.map((p) => `L ${xFor(p.m).toFixed(1)} ${yFor(p.v).toFixed(1)}`).join(' ') +
          ` L ${xFor(seg[seg.length - 1].m).toFixed(1)} ${yFor(CUSHION).toFixed(1)} Z`;
    const first = pts.find((p) => p.v <= 0.5 && (side === 'short' ? p.m > 0 : p.m < 0));
    const wipe = side === 'short' ? first : [...pts].reverse().find((p) => p.v <= 0.5 && p.m < 0);
    return { linePath: line, gainPath: area(above), lossPath: area(below), wipeout: wipe?.m ?? null };
  }, [side]);

  const gainSide = side === 'short' ? -1 : 1;
  const labelX = xFor(gainSide * 55);
  const labelY = yFor(payout(side, gainSide * 55)) - 10;
  const lossX = xFor(-gainSide * 55);
  const lossY = yFor(payout(side, -gainSide * 55)) + 18;

  return (
    <ExplainerPanel
      title="What you get back"
      tag={side === 'short' ? '100 τ cushion · 1x' : '100 τ cushion · 2x'}
      caption={
        side === 'short'
          ? 'Put in 100 τ. If alpha falls you get more back; if alpha rises you get less. Near a doubling the cushion is gone and the line hits zero.'
          : 'Put in 100 τ. If alpha rises you get more back, twice as fast; if alpha falls you lose twice as fast. Near a halving the cushion is gone and the line hits zero.'
      }
    >
      <div className="mb-5">
        <ExplainerToggle
          label="side"
          options={[
            { id: 'short', label: 'short' },
            { id: 'long', label: 'long' },
          ]}
          value={side}
          onChange={setSide}
        />
      </div>
      <svg
        viewBox="0 0 760 300"
        className="w-full"
        role="img"
        aria-label={`Value returned to the owner of a ${side} with a 100 TAO cushion, plotted against the alpha price move from minus 100 to plus 100 percent. The line crosses 100 TAO at zero move. ${side === 'short' ? 'It rises as alpha falls and reaches zero near a doubling.' : 'It rises as alpha rises and reaches zero as alpha approaches zero.'}`}
      >
        {/* Gain and loss areas relative to the cushion */}
        <path d={gainPath} fill="rgba(41,41,41,0.07)" />
        <path d={lossPath} fill={ACCENT_REGION} />

        {/* Axes */}
        <line x1={X0} y1={Y0} x2={X0} y2={BASE} stroke={INK} strokeWidth={1} />
        <line x1={X0} y1={BASE} x2={X0 + W} y2={BASE} stroke={INK} strokeWidth={1} />
        {[0, 100, 200].map((v) => (
          <g key={v}>
            <text {...FONT} x={X0 - 8} y={yFor(v) + 3} textAnchor="end" fill={INK_FAINT}>
              {v} τ
            </text>
          </g>
        ))}
        {[-100, -50, 0, 50, 100].map((m) => (
          <text key={m} {...FONT} x={xFor(m)} y={BASE + 16} textAnchor="middle" fill={INK_FAINT}>
            {m > 0 ? `+${m}` : m}%
          </text>
        ))}
        <text {...FONT} x={X0 + W} y={BASE + 34} textAnchor="end" fill={INK_FAINT}>
          ALPHA PRICE MOVE →
        </text>
        {/* Your cushion, unchanged */}
        <line x1={X0} y1={yFor(CUSHION)} x2={X0 + W} y2={yFor(CUSHION)} stroke={INK_FAINT} strokeWidth={1} strokeDasharray="4 3" />
        <text {...FONT} x={X0 + W - 4} y={yFor(CUSHION) - 6} textAnchor="end" fill={INK_FAINT}>
          YOUR 100 τ, UNCHANGED
        </text>

        {/* Open price */}
        <line x1={xFor(0)} y1={Y0} x2={xFor(0)} y2={BASE} stroke={INK_FAINT} strokeWidth={1} strokeDasharray="2 3" />
        <text {...FONT} x={xFor(0)} y={Y0 - 10} textAnchor="middle" fill={INK_FAINT}>
          OPEN PRICE
        </text>

        {/* Payoff line */}
        <path d={linePath} fill="none" stroke={INK} strokeWidth={2} />
        <circle cx={xFor(0)} cy={yFor(payout(side, 0))} r={3.5} fill="var(--bt-bg, #fff)" stroke={INK} strokeWidth={1.5} />

        <text {...FONT} x={labelX} y={labelY} textAnchor="middle" fill={INK} fontWeight={600}>
          GAIN
        </text>
        <text {...FONT} x={lossX} y={lossY} textAnchor="middle" fill={ACCENT} fontWeight={600}>
          LOSS
        </text>

        {wipeout !== null && (
          <g>
            <circle cx={xFor(wipeout)} cy={BASE} r={4} fill={ACCENT} />
            <text
              {...FONT}
              x={xFor(wipeout) + (side === 'short' ? -10 : 10)}
              y={BASE - 42}
              textAnchor={side === 'short' ? 'end' : 'start'}
              fill={ACCENT}
            >
              CUSHION GONE
            </text>
          </g>
        )}
      </svg>
      <p className="mt-3 text-[0.6875rem] leading-relaxed text-mute">
        A short runs at 1x and moves one-for-one against alpha; a long runs at 2x and moves two-for-one with
        it. Past the point where the cushion is gone the position is underwater: settlement pays you nothing,
        gives the pool whatever is left, and the pool carries the shortfall — which is why the pool lends at
        most 10% of itself per side. Example pool: 10,000 τ / 200,000 α, closed after one day (fee about
        0.06 τ on the short, 0.02 τ on the long).
      </p>
    </ExplainerPanel>
  );
}
