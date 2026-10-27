'use client';

import { useMemo, useRef, useState } from 'react';
import {
  Chart as ChartJS,
  CategoryScale,
  LinearScale,
  PointElement,
  LineElement,
  Filler,
  Tooltip,
  type Plugin,
} from 'chart.js';
import { Line } from 'react-chartjs-2';
import { ExplainerPanel, ExplainerSlider, ExplainerStat } from './explainer-panel';
import { AXIS_BORDER, GRAPH_FONT, GRID, INK, INK_FAINT, axisTitle, baseTicks } from './chart-theme';

ChartJS.register(CategoryScale, LinearScale, PointElement, LineElement, Filler, Tooltip);

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

  // The plugin is registered once at chart creation, so it reads live values
  // through a ref instead of closing over state that would go stale.
  const drawState = useRef({ tempo });
  drawState.current = { tempo };

  // Direct uppercase labels replacing the legend: one on the accruing ramp,
  // one beside the first epoch-boundary diamond.
  const annotationPlugin = useMemo<Plugin<'line'>>(
    () => ({
      id: 'tempoAnnotations',
      afterDatasetsDraw(chart) {
        const { tempo } = drawState.current;
        const { ctx, scales } = chart;
        const xScale = scales.x;
        const yScale = scales.y;
        if (!xScale || !yScale) return;

        ctx.save();
        ctx.font = GRAPH_FONT;

        // Up-left of the midpoint of the first ramp, in the empty region
        // above the rising line.
        ctx.fillStyle = INK;
        ctx.textAlign = 'right';
        ctx.fillText(
          'PENDING EMISSION',
          xScale.getPixelForValue(tempo * 0.55) - 8,
          yScale.getPixelForValue(tempo * 0.55) - 6,
        );

        // Beside the first boundary diamond
        ctx.fillStyle = INK_FAINT;
        ctx.textAlign = 'left';
        ctx.fillText(
          'EPOCH FIRES',
          xScale.getPixelForValue(tempo) + 8,
          yScale.getPixelForValue(tempo) + 4,
        );

        ctx.restore();
      },
    }),
    [],
  );

  const data = useMemo(
    () => ({
      datasets: [
        {
          label: 'Pending emission',
          data: sawtooth,
          borderColor: INK,
          backgroundColor: 'rgba(41, 41, 41, 0.03)',
          fill: true,
          tension: 0,
          pointRadius: 0,
          borderWidth: 1.5,
        },
        {
          label: 'Epoch fires',
          data: boundaries,
          borderColor: INK,
          backgroundColor: INK,
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
              return `Block ${block.toLocaleString()} (${formatDuration(block * BLOCK_SECONDS)} in)`;
            },
            label: (ctx: {parsed: {y: number}; datasetIndex: number}) =>
              ctx.datasetIndex === 1
                ? `epoch fires: ${ctx.parsed.y.toLocaleString()} α distributed`
                : `pending ${Math.round(ctx.parsed.y).toLocaleString()} α`,
          },
        },
      },
      scales: {
        x: {
          type: 'linear' as const,
          min: 0,
          max: EPOCHS_SHOWN * tempo,
          grid: {color: GRID},
          border: {color: AXIS_BORDER},
          ticks: baseTicks({
            callback: (value: string | number) => Number(value).toLocaleString(),
          }),
          title: axisTitle('blocks since last epoch reset'),
        },
        y: {
          min: 0,
          grid: {color: GRID},
          border: {color: AXIS_BORDER},
          ticks: baseTicks({maxTicksLimit: 5}),
          title: axisTitle('pending emission (α, at 1 α/block)'),
        },
      },
    }),
    [tempo],
  );

  return (
    <ExplainerPanel
      title="tempo epoch timeline"
      caption={
        <>
          Emission accrues every block and pays out when the epoch fires:{' '}
          <a
            href="/code/pallets/subtensor/src/coinbase/run_coinbase.rs#L1117-L1132"
            className="underline"
          >
            should_run_epoch (coinbase/run_coinbase.rs)
          </a>{' '}
          triggers once current_block − LastEpochBlock ≥ tempo. Three epochs shown at an
          illustrative 1 α/block; each diamond is an epoch boundary where Yuma Consensus runs
          and the accumulated alpha is distributed.
        </>
      }
    >
      <div className="h-52">
        <Line data={data} options={options} plugins={[annotationPlugin]} />
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
        <ExplainerStat
          label="Epoch length"
          value={formatDuration(tempo * BLOCK_SECONDS)}
          hint={`${tempo.toLocaleString()} blocks × 12 s`}
        />
        <ExplainerStat
          label="Epochs per day"
          value={
            BLOCKS_PER_DAY / tempo >= 1
              ? (BLOCKS_PER_DAY / tempo).toFixed(1)
              : (BLOCKS_PER_DAY / tempo).toFixed(2)
          }
          hint="7,200 blocks per day"
        />
        <ExplainerStat
          label="Mainnet default"
          value="360 blocks"
          hint="~72 minutes per epoch"
        />
        <ExplainerStat
          label="Owner-settable range"
          value="360 – 50,400"
          hint="~72 minutes to ~7 days"
        />
      </div>

      <div className="mt-5">
        <ExplainerSlider
          label="tempo (blocks per epoch)"
          value={tempo}
          min={360}
          max={50_400}
          step={360}
          display={`${tempo.toLocaleString()} blocks`}
          onChange={setTempo}
        />
      </div>
    </ExplainerPanel>
  );
}
