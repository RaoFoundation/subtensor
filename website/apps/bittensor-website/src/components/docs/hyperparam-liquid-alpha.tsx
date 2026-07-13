'use client';

import { useMemo, useState } from 'react';
import {
  Chart as ChartJS,
  CategoryScale,
  LinearScale,
  PointElement,
  LineElement,
  Filler,
  Tooltip,
  Legend,
} from 'chart.js';
import { Line } from 'react-chartjs-2';
import { ExplainerPanel, ExplainerSlider, ExplainerStat } from './explainer-panel';

ChartJS.register(CategoryScale, LinearScale, PointElement, LineElement, Filler, Tooltip, Legend);

const SAMPLE_POINTS = 100;
// Chain floor for alpha_low/alpha_high: u16::MAX / 40 = 1638 ≈ 0.025.
const ALPHA_FLOOR = 0.025;

// Mirrors Pallet::alpha_sigmoid in pallets/subtensor/src/epoch/run_epoch.rs:
// sigmoid = 1 / (1 + e^(-(steepness/100) * (diff - 0.5))), alpha clamped to [low, high].
function alphaSigmoid(diff: number, low: number, high: number, steepness: number): number {
  const sigmoid = 1 / (1 + Math.exp((-steepness / 100) * (diff - 0.5)));
  const alpha = low + sigmoid * (high - low);
  return Math.min(Math.max(alpha, low), high);
}

export function HyperparamLiquidAlpha({ focus }: { focus?: string }) {
  const [enabled, setEnabled] = useState(true);
  const [alphaLow, setAlphaLow] = useState(0.7);
  const [alphaHigh, setAlphaHigh] = useState(0.9);
  const [steepness, setSteepness] = useState(1000);
  const [bondsMovingAvg, setBondsMovingAvg] = useState(0.9);

  // Flat EMA rate used when liquid alpha is off: 1 - bonds_moving_avg / 1e6.
  const flatAlpha = 1 - bondsMovingAvg;

  // The chain forbids alpha_low > alpha_high, so the sliders drag each other.
  const changeLow = (v: number) => {
    setAlphaLow(v);
    if (v > alphaHigh) setAlphaHigh(v);
  };
  const changeHigh = (v: number) => {
    setAlphaHigh(v);
    if (v < alphaLow) setAlphaLow(v);
  };

  const curve = useMemo(() => {
    const xs = Array.from({ length: SAMPLE_POINTS + 1 }, (_, i) => i / SAMPLE_POINTS);
    const ys = xs.map((x) => (enabled ? alphaSigmoid(x, alphaLow, alphaHigh, steepness) : flatAlpha));
    return { xs, ys };
  }, [enabled, alphaLow, alphaHigh, steepness, flatAlpha]);

  const data = useMemo(
    () => ({
      labels: curve.xs.map((x) => x.toFixed(2)),
      datasets: [
        {
          label: enabled ? 'per-pair EMA rate (liquid alpha)' : 'flat EMA rate (bonds_moving_avg)',
          data: curve.ys,
          borderColor: 'rgb(41, 41, 41)',
          backgroundColor: 'rgba(41, 41, 41, 0.08)',
          fill: true,
          tension: 0,
          pointRadius: 0,
          borderWidth: 1.5,
        },
        ...(enabled
          ? [
              {
                label: 'flat rate if disabled',
                data: curve.xs.map(() => flatAlpha),
                borderColor: 'rgba(41, 41, 41, 0.35)',
                borderDash: [4, 4],
                fill: false,
                tension: 0,
                pointRadius: 0,
                borderWidth: 1,
              },
            ]
          : []),
      ],
    }),
    [curve, enabled, flatAlpha],
  );

  const options = useMemo(
    () => ({
      responsive: true,
      maintainAspectRatio: false,
      interaction: { mode: 'index' as const, intersect: false },
      plugins: {
        legend: { display: false },
        tooltip: {
          callbacks: {
            title: (items: { dataIndex: number }[]) => {
              const idx = items[0]?.dataIndex ?? 0;
              return `deviation ${curve.xs[idx]?.toFixed(2)}`;
            },
            label: (ctx: { parsed: { y: number } }) => `alpha ${ctx.parsed.y.toFixed(3)}`,
          },
        },
      },
      scales: {
        x: {
          grid: { color: 'rgba(41, 41, 41, 0.06)' },
          ticks: { maxTicksLimit: 11, font: { family: 'FiraCode, monospace', size: 10 } },
          title: { display: true, text: 'deviation from consensus (combined_diff)', font: { size: 11 } },
        },
        y: {
          min: 0,
          max: 1,
          grid: { color: 'rgba(41, 41, 41, 0.06)' },
          ticks: { font: { family: 'FiraCode, monospace', size: 10 } },
          title: { display: true, text: 'bonds EMA rate (alpha)', font: { size: 11 } },
        },
      },
    }),
    [curve.xs],
  );

  const focusClass = (name: string) => (focus === name ? 'border border-line bg-bg p-3' : '');

  return (
    <ExplainerPanel
      title="Liquid alpha: per-weight bonds EMA rate"
      caption="Matches alpha_sigmoid in the epoch code. Deviation is weight − consensus when buying bond, bond − weight when selling. Higher alpha moves bonds faster. Requires yuma3_enabled; when liquid alpha is off, every pair uses the flat 1 − bonds_moving_avg / 1e6."
    >
      <div className="h-52">
        <Line data={data} options={options} />
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-3">
        <ExplainerStat
          label="In consensus (diff = 0)"
          value={curve.ys[0]?.toFixed(3) ?? '—'}
          hint="EMA rate for weights matching consensus"
        />
        <ExplainerStat
          label="Max deviation (diff = 1)"
          value={curve.ys[SAMPLE_POINTS]?.toFixed(3) ?? '—'}
          hint="EMA rate at full deviation"
        />
        <ExplainerStat
          label="Flat rate when disabled"
          value={flatAlpha.toFixed(3)}
          hint={`1 − ${Math.round(bondsMovingAvg * 1_000_000).toLocaleString()} / 1,000,000`}
        />
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-2">
        <div className={focusClass('liquid_alpha_enabled')}>
          <label className="flex items-center gap-2">
            <input
              type="checkbox"
              checked={enabled}
              onChange={(e) => setEnabled(e.target.checked)}
              className="accent-[var(--bt-fg)]"
            />
            <span className="bt-label text-mute">liquid_alpha_enabled</span>
            <span className="font-mono text-xs">{enabled ? 'true' : 'false'}</span>
          </label>
        </div>
        <div className={focusClass('bonds_moving_avg')}>
          <ExplainerSlider
            label="bonds_moving_avg"
            value={bondsMovingAvg}
            min={0}
            max={0.995}
            step={0.005}
            display={`${Math.round(bondsMovingAvg * 1_000_000).toLocaleString()} (${bondsMovingAvg.toFixed(3)})`}
            onChange={setBondsMovingAvg}
          />
        </div>
        <div className={focusClass('alpha_low')}>
          <ExplainerSlider
            label="alpha_low"
            value={alphaLow}
            min={ALPHA_FLOOR}
            max={1}
            step={0.005}
            display={`${Math.round(alphaLow * 65535).toLocaleString()} (${alphaLow.toFixed(3)})`}
            onChange={changeLow}
          />
        </div>
        <div className={focusClass('alpha_high')}>
          <ExplainerSlider
            label="alpha_high"
            value={alphaHigh}
            min={ALPHA_FLOOR}
            max={1}
            step={0.005}
            display={`${Math.round(alphaHigh * 65535).toLocaleString()} (${alphaHigh.toFixed(3)})`}
            onChange={changeHigh}
          />
        </div>
        <div className={focusClass('alpha_sigmoid_steepness')}>
          <ExplainerSlider
            label="alpha_sigmoid_steepness"
            value={steepness}
            min={-3000}
            max={3000}
            step={50}
            display={`${steepness} (slope ${(steepness / 100).toFixed(1)}; negative is root-only)`}
            onChange={setSteepness}
          />
        </div>
      </div>
    </ExplainerPanel>
  );
}
