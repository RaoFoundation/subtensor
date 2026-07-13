'use client';

import { useMemo, useState } from 'react';
import {
  Chart as ChartJS,
  CategoryScale,
  LinearScale,
  BarElement,
  PointElement,
  LineElement,
  Filler,
  Tooltip,
  Legend,
} from 'chart.js';
import { Chart } from 'react-chartjs-2';
import { ExplainerPanel, ExplainerSlider, ExplainerStat } from './explainer-panel';

ChartJS.register(
  CategoryScale,
  LinearScale,
  BarElement,
  PointElement,
  LineElement,
  Filler,
  Tooltip,
  Legend,
);

const BLOCK_SECONDS = 12;

// Illustrative snapshot: blocks since each validator last set weights
// (current_block − last_update), spread from very fresh to badly stale.
const WEIGHT_AGES = [
  120, 340, 760, 1_150, 1_900, 2_600, 3_400, 4_200, 4_900, 5_600, 6_800, 8_500, 11_000, 14_000,
];

function formatDuration(blocks: number): string {
  const seconds = blocks * BLOCK_SECONDS;
  if (seconds < 3600) return `${(seconds / 60).toFixed(0)} min`;
  if (seconds < 86_400) return `${(seconds / 3600).toFixed(1)} h`;
  return `${(seconds / 86_400).toFixed(1)} days`;
}

export function HyperparamActivityCutoffChart() {
  const [cutoff, setCutoff] = useState(5000);

  // run_epoch.rs: inactive when last_update + activity_cutoff < current_block,
  // i.e. a validator survives while its age is at most the cutoff.
  const inactive = useMemo(() => WEIGHT_AGES.map((age) => age > cutoff), [cutoff]);
  const activeCount = inactive.filter((i) => !i).length;

  const data = useMemo(
    () => ({
      labels: WEIGHT_AGES.map((_, i) => `v${i + 1}`),
      datasets: [
        {
          type: 'line' as const,
          label: 'activity_cutoff',
          data: WEIGHT_AGES.map(() => cutoff),
          borderColor: 'rgb(41, 41, 41)',
          borderDash: [6, 4],
          borderWidth: 1.5,
          pointRadius: 0,
          fill: false,
        },
        {
          type: 'bar' as const,
          label: 'Blocks since last weights',
          data: [...WEIGHT_AGES],
          backgroundColor: inactive.map((isInactive) =>
            isInactive ? 'rgba(41, 41, 41, 0.12)' : 'rgba(41, 41, 41, 0.65)',
          ),
          borderColor: 'rgb(41, 41, 41)',
          borderWidth: 1,
        },
      ],
    }),
    [cutoff, inactive],
  );

  const options = useMemo(
    () => ({
      responsive: true,
      maintainAspectRatio: false,
      interaction: {mode: 'index' as const, intersect: false},
      plugins: {
        legend: {display: false},
        tooltip: {
          callbacks: {
            title: (items: {dataIndex: number}[]) => {
              const idx = items[0]?.dataIndex ?? 0;
              return `Validator ${idx + 1}`;
            },
            label: (ctx: {parsed: {y: number}; datasetIndex: number; dataIndex: number}) => {
              if (ctx.datasetIndex === 0) return `cutoff ${cutoff} blocks`;
              const status = inactive[ctx.dataIndex] ? 'inactive — stake masked' : 'active';
              return `${ctx.parsed.y} blocks since weights — ${status}`;
            },
          },
        },
      },
      scales: {
        x: {
          grid: {color: 'rgba(41, 41, 41, 0.06)'},
          ticks: {font: {family: 'FiraCode, monospace', size: 10}},
          title: {display: true, text: 'validators', font: {size: 11}},
        },
        y: {
          min: 0,
          grid: {color: 'rgba(41, 41, 41, 0.06)'},
          ticks: {font: {family: 'FiraCode, monospace', size: 10}},
          title: {display: true, text: 'blocks since last weight set', font: {size: 11}},
        },
      },
    }),
    [cutoff, inactive],
  );

  return (
    <ExplainerPanel
      title="activity_cutoff inactivity mask"
      caption="Each bar is how long a validator has gone without setting weights. run_epoch.rs marks a neuron inactive when last_update + activity_cutoff < current_block: bars past the dashed line fade out — their stake is masked from the active-stake vector and they earn no dividends until they set weights again."
    >
      <div className="h-52">
        <Chart type="bar" data={data} options={options} />
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-3">
        <ExplainerStat
          label="Active validators"
          value={`${activeCount} / ${WEIGHT_AGES.length}`}
          hint="counted into consensus"
        />
        <ExplainerStat
          label="Cutoff in wall-clock"
          value={formatDuration(cutoff)}
          hint={`${cutoff} blocks × 12 s`}
        />
        <ExplainerStat
          label="Legacy default"
          value="5,000 blocks"
          hint="~16.7 h at tempo 360"
        />
      </div>

      <div className="mt-5">
        <ExplainerSlider
          label="activity_cutoff (blocks)"
          value={cutoff}
          min={500}
          max={15000}
          step={250}
          display={`${cutoff} blocks`}
          onChange={setCutoff}
        />
      </div>
    </ExplainerPanel>
  );
}
