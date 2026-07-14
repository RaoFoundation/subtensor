'use client';

import { useMemo, useState } from 'react';
import {
  Chart as ChartJS,
  CategoryScale,
  LinearScale,
  BarElement,
  PointElement,
  LineElement,
  Tooltip,
  Legend,
} from 'chart.js';
import { Chart } from 'react-chartjs-2';
import { ExplainerPanel, ExplainerSlider, ExplainerStat } from './explainer-panel';
import { AXIS_BORDER, GRID, INK, axisTitle, baseTicks } from './chart-theme';

ChartJS.register(CategoryScale, LinearScale, BarElement, PointElement, LineElement, Tooltip, Legend);

const MINERS = ['M1', 'M2', 'M3', 'M4', 'M5'];
// Stake-weighted consensus per miner (what the majority of stake endorses).
const CONSENSUS = [0.3, 0.25, 0.2, 0.15, 0.1];
// An in-consensus validator weights exactly at consensus; clipping never touches it.
const HONEST = [0.3, 0.25, 0.2, 0.15, 0.1];
// A deviant validator goes all-in on M5, far above its consensus level.
const DEVIANT = [0.05, 0.1, 0.05, 0.1, 0.7];

// Mirrors inplace_col_clip + interpolate in pallets/subtensor/src/epoch/run_epoch.rs:
// weights_for_bonds = (1 - penalty) * weights + penalty * min(weights, consensus).
function bondWeights(raw: number[], penalty: number): number[] {
  return raw.map((w, j) => (1 - penalty) * w + penalty * Math.min(w, CONSENSUS[j]));
}

export function HyperparamBondsPenaltyChart() {
  const [penalty, setPenalty] = useState(1);

  const deviant = useMemo(() => bondWeights(DEVIANT, penalty), [penalty]);
  const honest = useMemo(() => bondWeights(HONEST, penalty), [penalty]);

  const data = useMemo(
    () => ({
      labels: MINERS,
      datasets: [
        {
          type: 'bar' as const,
          label: 'in-consensus validator (bond weights)',
          data: honest,
          backgroundColor: 'rgba(41, 41, 41, 0.3)',
          borderColor: INK,
          borderWidth: 1,
        },
        {
          type: 'bar' as const,
          label: 'deviant validator (bond weights)',
          data: deviant,
          backgroundColor: 'rgba(41, 41, 41, 0.85)',
          borderColor: INK,
          borderWidth: 1,
        },
        {
          type: 'line' as const,
          label: 'consensus clip level',
          data: CONSENSUS,
          borderColor: 'rgba(41, 41, 41, 0.6)',
          borderDash: [4, 4],
          borderWidth: 1,
          pointRadius: 2,
          pointBackgroundColor: INK,
          stepped: 'middle' as const,
          fill: false,
        },
      ],
    }),
    [honest, deviant],
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
            label: (ctx: { dataset: { label?: string }; parsed: { y: number } }) =>
              `${ctx.dataset.label ?? ''}: ${ctx.parsed.y.toFixed(3)}`,
          },
        },
      },
      scales: {
        x: {
          grid: { color: GRID },
          border: { color: AXIS_BORDER },
          ticks: baseTicks(),
          title: axisTitle('miner'),
        },
        y: {
          min: 0,
          max: 0.75,
          grid: { color: GRID },
          border: { color: AXIS_BORDER },
          ticks: baseTicks({ maxTicksLimit: 5 }),
          title: axisTitle('weight used for bond accrual'),
        },
      },
    }),
    [],
  );

  const deviantMass = deviant.reduce((a, b) => a + b, 0);
  const rawMass = DEVIANT.reduce((a, b) => a + b, 0);

  return (
    <ExplainerPanel
      title="bonds_penalty: interpolating raw and clipped weights"
      caption="Matches weights_for_bonds = interpolate(weights, clipped_weights, bonds_penalty) in run_epoch.rs. The dashed steps are the stake-weighted consensus; clipping caps each weight there. An in-consensus validator (light bars) is never affected — only weight above consensus is at stake."
    >
      <div className="h-52">
        <Chart type="bar" data={data} options={options} />
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-3">
        <ExplainerStat
          label="Deviant bond weight on M5"
          value={`${DEVIANT[4].toFixed(2)} → ${deviant[4].toFixed(3)}`}
          hint={`consensus caps M5 at ${CONSENSUS[4].toFixed(2)}`}
        />
        <ExplainerStat
          label="Deviant bond mass retained"
          value={`${((deviantMass / rawMass) * 100).toFixed(0)}%`}
          hint="share of raw weight still accruing bonds"
        />
        <ExplainerStat
          label="In-consensus validator"
          value={`${(honest.reduce((a, b) => a + b, 0) * 100).toFixed(0)}% retained`}
          hint="weights at or below consensus are never clipped"
        />
      </div>

      <div className="mt-5">
        <ExplainerSlider
          label="bonds_penalty"
          value={penalty}
          min={0}
          max={1}
          step={0.05}
          display={`${Math.round(penalty * 65535).toLocaleString()} (${penalty.toFixed(2)})`}
          onChange={setPenalty}
        />
      </div>
    </ExplainerPanel>
  );
}
