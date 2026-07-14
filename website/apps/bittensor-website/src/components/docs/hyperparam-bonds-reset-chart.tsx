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
import { AXIS_BORDER, GRID, INK, INK_FAINT, axisTitle, baseTicks } from './chart-theme';

ChartJS.register(CategoryScale, LinearScale, PointElement, LineElement, Filler, Tooltip, Legend);

const EPOCHS = 40;
const COMMIT_EPOCH = 20;
const TARGET_BOND = 1; // validator keeps endorsing the miner at full instant bond

// Bonds accrue via B(t) = alpha * target + (1 - alpha) * B(t-1); do_reset_bonds
// (run_epoch.rs) zeroes the bond at the metadata-commit epoch when the flag is on.
function bondSeries(alpha: number, resetOn: boolean): number[] {
  const ys: number[] = [];
  let b = 0;
  for (let t = 0; t <= EPOCHS; t++) {
    if (resetOn && t === COMMIT_EPOCH) b = 0;
    ys.push(b);
    b = alpha * TARGET_BOND + (1 - alpha) * b;
  }
  return ys;
}

export function HyperparamBondsResetChart() {
  const [resetOn, setResetOn] = useState(true);
  const [bondsMovingAvg, setBondsMovingAvg] = useState(0.9);

  const alpha = 1 - bondsMovingAvg;
  const withFlag = useMemo(() => bondSeries(alpha, resetOn), [alpha, resetOn]);
  const noReset = useMemo(() => bondSeries(alpha, false), [alpha]);

  const data = useMemo(
    () => ({
      labels: withFlag.map((_, t) => `${t}`),
      datasets: [
        {
          label: resetOn ? 'bond on committing miner (reset on)' : 'bond on committing miner',
          data: withFlag,
          borderColor: INK,
          backgroundColor: 'rgba(41, 41, 41, 0.03)',
          fill: true,
          tension: 0,
          pointRadius: 0,
          borderWidth: 1.75,
        },
        ...(resetOn
          ? [
              {
                label: 'same bond if reset were off',
                data: noReset,
                borderColor: INK_FAINT,
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
    [withFlag, noReset, resetOn],
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
              const t = items[0]?.dataIndex ?? 0;
              return t === COMMIT_EPOCH ? `epoch ${t} — metadata commit` : `epoch ${t}`;
            },
            label: (ctx: { parsed: { y: number } }) => `bond ${ctx.parsed.y.toFixed(3)}`,
          },
        },
      },
      scales: {
        x: {
          grid: { color: GRID },
          border: { color: AXIS_BORDER },
          ticks: baseTicks(),
          title: axisTitle(`epoch (metadata commit at epoch ${COMMIT_EPOCH})`),
        },
        y: {
          min: 0,
          max: 1,
          grid: { color: GRID },
          border: { color: AXIS_BORDER },
          ticks: baseTicks({ maxTicksLimit: 5 }),
          title: axisTitle('bond on the committing miner'),
        },
      },
    }),
    [],
  );

  const atCommit = withFlag[COMMIT_EPOCH] ?? 0;
  const beforeCommit = withFlag[COMMIT_EPOCH - 1] ?? 0;
  const atEnd = withFlag[EPOCHS] ?? 0;

  return (
    <ExplainerPanel
      title="bonds_reset_enabled: bonds wiped at a metadata commit"
      caption="A validator keeps endorsing one miner, so its bond accrues via the EMA B(t) = alpha × ΔB + (1 − alpha) × B(t−1). When the miner commits metadata and the flag is on, do_reset_bonds erases every bond pointing at it (run_epoch.rs) and the EMA rebuilds from zero; when off, the commit changes nothing."
    >
      <div className="h-52">
        <Line data={data} options={options} />
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-3">
        <ExplainerStat
          label={`Bond at commit (epoch ${COMMIT_EPOCH})`}
          value={resetOn ? `${beforeCommit.toFixed(3)} → 0.000` : beforeCommit.toFixed(3)}
          hint={resetOn ? 'do_reset_bonds drops the column to zero' : 'flag off — commit has no effect'}
        />
        <ExplainerStat
          label={`Bond at epoch ${EPOCHS}`}
          value={atEnd.toFixed(3)}
          hint={resetOn ? 'rebuilt through the normal EMA' : 'uninterrupted accrual'}
        />
        <ExplainerStat
          label="EMA rate (alpha)"
          value={alpha.toFixed(3)}
          hint={`1 − ${Math.round(bondsMovingAvg * 1_000_000).toLocaleString()} / 1,000,000`}
        />
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-2">
        <label className="flex items-center gap-2">
          <input
            type="checkbox"
            checked={resetOn}
            onChange={(e) => setResetOn(e.target.checked)}
            className="accent-[var(--bt-fg)]"
          />
          <span className="bt-label text-mute">bonds_reset_enabled</span>
          <span className="font-mono text-xs">{resetOn ? 'true' : 'false'}</span>
        </label>
        <ExplainerSlider
          label="bonds_moving_avg (rebuild speed)"
          value={bondsMovingAvg}
          min={0.5}
          max={0.99}
          step={0.01}
          display={`${Math.round(bondsMovingAvg * 1_000_000).toLocaleString()} (${bondsMovingAvg.toFixed(2)})`}
          onChange={setBondsMovingAvg}
        />
      </div>
    </ExplainerPanel>
  );
}
