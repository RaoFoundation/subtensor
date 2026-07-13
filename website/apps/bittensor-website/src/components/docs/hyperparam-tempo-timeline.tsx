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

const BLOCK_SECONDS = 12;
const BLOCKS_PER_DAY = 86_400 / BLOCK_SECONDS;
const EPOCHS_SHOWN = 3;

function formatDuration(seconds: number): string {
  if (seconds < 3600) return `${(seconds / 60).toFixed(0)} min`;
  if (seconds < 86_400) return `${(seconds / 3600).toFixed(1)} h`;
  return `${(seconds / 86_400).toFixed(1)} days`;
}

export function HyperparamTempoTimeline() {
  const [tempo, setTempo] = useState(360);

  // Sawtooth: emission accrues 1 α/block within an epoch and pays out at the
  // boundary, mirroring should_run_epoch's `current_block − LastEpochBlock ≥ tempo`.
  const sawtooth = useMemo(() => {
    const points: {x: number; y: number}[] = [];
    for (let e = 0; e < EPOCHS_SHOWN; e++) {
      points.push({x: e * tempo, y: 0});
      points.push({x: (e + 1) * tempo, y: tempo});
    }
    points.push({x: EPOCHS_SHOWN * tempo, y: 0});
    return points;
  }, [tempo]);

  const boundaries = useMemo(
    () =>
      Array.from({length: EPOCHS_SHOWN}, (_, e) => ({x: (e + 1) * tempo, y: tempo})),
    [tempo],
  );

  const data = useMemo(
    () => ({
      datasets: [
        {
          label: 'Pending emission',
          data: sawtooth,
          borderColor: 'rgb(41, 41, 41)',
          backgroundColor: 'rgba(41, 41, 41, 0.08)',
          fill: true,
          tension: 0,
          pointRadius: 0,
          borderWidth: 1.5,
        },
        {
          label: 'Epoch fires',
          data: boundaries,
          borderColor: 'rgb(41, 41, 41)',
          backgroundColor: 'rgb(41, 41, 41)',
          showLine: false,
          pointRadius: 3,
          pointStyle: 'rectRot' as const,
        },
      ],
    }),
    [sawtooth, boundaries],
  );

  const options = useMemo(
    () => ({
      responsive: true,
      maintainAspectRatio: false,
      interaction: {mode: 'nearest' as const, intersect: false},
      plugins: {
        legend: {display: false},
        tooltip: {
          callbacks: {
            title: (items: {parsed: {x: number}}[]) => {
              const block = items[0]?.parsed.x ?? 0;
              return `Block ${block} (${formatDuration(block * BLOCK_SECONDS)} in)`;
            },
            label: (ctx: {parsed: {y: number}; datasetIndex: number}) =>
              ctx.datasetIndex === 1
                ? `epoch fires: ${ctx.parsed.y} α distributed`
                : `pending ${ctx.parsed.y.toFixed(0)} α`,
          },
        },
      },
      scales: {
        x: {
          type: 'linear' as const,
          min: 0,
          max: EPOCHS_SHOWN * tempo,
          grid: {color: 'rgba(41, 41, 41, 0.06)'},
          ticks: {
            maxTicksLimit: 10,
            font: {family: 'FiraCode, monospace', size: 10},
            callback: (value: string | number) => String(value),
          },
          title: {display: true, text: 'blocks since last epoch reset', font: {size: 11}},
        },
        y: {
          min: 0,
          grid: {color: 'rgba(41, 41, 41, 0.06)'},
          ticks: {font: {family: 'FiraCode, monospace', size: 10}},
          title: {display: true, text: 'pending emission (α, at 1 α/block)', font: {size: 11}},
        },
      },
    }),
    [tempo],
  );

  return (
    <ExplainerPanel
      title="tempo epoch timeline"
      caption="Emission accrues every block and pays out when the epoch fires: should_run_epoch (coinbase/run_coinbase.rs) triggers once current_block − LastEpochBlock ≥ tempo. Three epochs shown at an illustrative 1 α/block; each diamond is an epoch boundary where Yuma Consensus runs and the accumulated alpha is distributed."
    >
      <div className="h-52">
        <Line data={data} options={options} />
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-3">
        <ExplainerStat
          label="Epoch length"
          value={formatDuration(tempo * BLOCK_SECONDS)}
          hint={`${tempo} blocks × 12 s`}
        />
        <ExplainerStat
          label="Epochs per day"
          value={(BLOCKS_PER_DAY / tempo).toFixed(1)}
          hint="7,200 blocks per day"
        />
        <ExplainerStat
          label="Mainnet default"
          value="360 blocks"
          hint="~72 minutes per epoch"
        />
      </div>

      <div className="mt-5">
        <ExplainerSlider
          label="tempo (blocks per epoch)"
          value={tempo}
          min={60}
          max={1440}
          step={30}
          display={`${tempo} blocks`}
          onChange={setTempo}
        />
      </div>
    </ExplainerPanel>
  );
}
