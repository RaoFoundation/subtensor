'use client';

import { useMemo, useState } from 'react';
import {
  Chart as ChartJS,
  CategoryScale,
  LinearScale,
  BarElement,
  Tooltip,
  Legend,
} from 'chart.js';
import { Bar } from 'react-chartjs-2';
import { ExplainerPanel, ExplainerSlider, ExplainerStat } from './explainer-panel';

ChartJS.register(CategoryScale, LinearScale, BarElement, Tooltip, Legend);

const U16_MAX = 65535;
const MINER_LABELS = ['uid 3', 'uid 7', 'uid 12', 'uid 21', 'uid 34'];

/** Largest cutoff c such that, after clipping every weight to c and
 * sum-normalizing, the largest share is <= limit. Mirrors the chain's
 * check (`check_vec_max_limited`) and the SDK's redistribution-preserving
 * clip (`clip_to_max_weight`). */
function clipToLimit(weights: number[], limit: number): number[] {
  const total = weights.reduce((a, b) => a + b, 0);
  if (total <= 0) return weights.map(() => 0);
  if (weights.length * limit <= 1) return weights.map(() => 1 / weights.length);

  const normalized = weights.map((w) => w / total);
  if (Math.max(...normalized) <= limit) return normalized;

  let lo = 0;
  let hi = Math.max(...weights);
  for (let i = 0; i < 50; i++) {
    const mid = (lo + hi) / 2;
    const clippedSum = weights.reduce((a, w) => a + Math.min(w, mid), 0);
    if (clippedSum > 0 && mid / clippedSum <= limit) lo = mid;
    else hi = mid;
  }
  const clipped = weights.map((w) => Math.min(w, lo));
  const clippedTotal = clipped.reduce((a, b) => a + b, 0);
  return clipped.map((w) => w / clippedTotal);
}

function ClippingPlayground({ focus }: { focus?: string }) {
  const [weights, setWeights] = useState([80, 35, 20, 10, 0]);
  const [limitRaw, setLimitRaw] = useState(19661); // ~0.3
  const [minAllowed, setMinAllowed] = useState(3);

  const limit = limitRaw / U16_MAX;
  const total = weights.reduce((a, b) => a + b, 0);
  const before = weights.map((w) => (total > 0 ? w / total : 0));
  const after = useMemo(() => clipToLimit(weights, limit), [weights, limit]);

  const nonzero = weights.filter((w) => w > 0).length;
  const meetsMin = nonzero >= minAllowed;
  const wouldClip = Math.max(...before) > limit + 1e-9;

  const data = useMemo(
    () => ({
      labels: MINER_LABELS,
      datasets: [
        {
          label: 'Submitted (normalized)',
          data: before,
          backgroundColor: 'rgba(41, 41, 41, 0.25)',
          borderColor: 'rgb(41, 41, 41)',
          borderWidth: 1,
        },
        {
          label: 'After clip + renormalize',
          data: after,
          backgroundColor: 'rgba(41, 41, 41, 0.75)',
          borderColor: 'rgb(41, 41, 41)',
          borderWidth: 1,
        },
      ],
    }),
    [before, after],
  );

  const options = useMemo(
    () => ({
      responsive: true,
      maintainAspectRatio: false,
      plugins: {
        legend: {
          labels: {font: {family: 'FiraCode, monospace', size: 10}, boxWidth: 12},
        },
        tooltip: {
          callbacks: {
            label: (ctx: {dataset: {label?: string}; parsed: {y: number}}) =>
              `${ctx.dataset.label}: ${(ctx.parsed.y * 100).toFixed(1)}%`,
          },
        },
      },
      scales: {
        x: {
          grid: {color: 'rgba(41, 41, 41, 0.06)'},
          ticks: {font: {family: 'FiraCode, monospace', size: 10}},
        },
        y: {
          min: 0,
          max: 1,
          grid: {color: 'rgba(41, 41, 41, 0.06)'},
          ticks: {
            font: {family: 'FiraCode, monospace', size: 10},
            callback: (value: string | number) => `${(Number(value) * 100).toFixed(0)}%`,
          },
          title: {display: true, text: 'share of total weight', font: {size: 11}},
        },
      },
    }),
    [],
  );

  const highlight = (name: string) =>
    focus === name ? 'border border-line bg-bg p-3' : undefined;

  return (
    <ExplainerPanel
      title="Weight clipping playground"
      caption="Drag the raw weights and the limit. The chain rejects submissions whose largest normalized share exceeds max_weights_limit; the SDK instead clips and renormalizes, redistributing the excess."
    >
      <div className="h-52">
        <Bar data={data} options={options} />
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-3">
        <ExplainerStat
          label="Largest share"
          value={`${(Math.max(...before) * 100).toFixed(1)}% → ${(Math.max(...after) * 100).toFixed(1)}%`}
          hint={wouldClip ? 'exceeds limit — clipped' : 'within limit — unchanged'}
        />
        <ExplainerStat
          label="max_weights_limit"
          value={`${limitRaw} / 65535 ≈ ${(limit * 100).toFixed(1)}%`}
          hint="u16 fraction; 65535 = no cap"
        />
        <ExplainerStat
          label="min_allowed_weights"
          value={`${nonzero} nonzero / ${minAllowed} required`}
          hint={meetsMin ? 'passes length check' : 'rejected: WeightVecLengthIsLow'}
        />
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-2">
        {weights.map((w, i) => (
          <ExplainerSlider
            key={MINER_LABELS[i]}
            label={`weight for ${MINER_LABELS[i]}`}
            value={w}
            min={0}
            max={100}
            step={1}
            display={String(w)}
            onChange={(value) =>
              setWeights((prev) => prev.map((p, j) => (j === i ? value : p)))
            }
          />
        ))}
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-2">
        <div className={highlight('max_weights_limit')}>
          <ExplainerSlider
            label="max_weights_limit"
            value={limitRaw}
            min={3277}
            max={U16_MAX}
            step={655}
            display={`≈ ${(limit * 100).toFixed(0)}%`}
            onChange={setLimitRaw}
          />
        </div>
        <div className={highlight('min_allowed_weights')}>
          <ExplainerSlider
            label="min_allowed_weights"
            value={minAllowed}
            min={1}
            max={5}
            step={1}
            display={String(minAllowed)}
            onChange={setMinAllowed}
          />
        </div>
      </div>
    </ExplainerPanel>
  );
}

const TEMPO_BLOCKS = 360;
const TIMELINE_EPOCHS = 8;

function epochState(epoch: number, period: number): 'commit' | 'hidden' | 'reveal' | 'expired' {
  if (epoch === 0) return 'commit';
  if (epoch < period) return 'hidden';
  if (epoch === period) return 'reveal';
  return 'expired';
}

const EPOCH_STYLE: Record<ReturnType<typeof epochState>, string> = {
  commit: 'border-[rgb(41,41,41)] bg-bg',
  hidden: 'border-line bg-[rgba(41,41,41,0.08)] text-mute',
  reveal: 'border-[rgb(41,41,41)] bg-[rgba(41,41,41,0.75)] text-white',
  expired: 'border-line bg-bg text-mute opacity-50',
};

const EPOCH_LABEL: Record<ReturnType<typeof epochState>, string> = {
  commit: 'commit',
  hidden: 'hidden',
  reveal: 'reveal',
  expired: 'expired',
};

function CommitRevealTimeline() {
  const [period, setPeriod] = useState(1);

  const delayBlocks = period * TEMPO_BLOCKS;
  const delayMinutes = (delayBlocks * 12) / 60;

  return (
    <ExplainerPanel
      title="Commit-reveal timeline"
      caption="A commit is tagged with its epoch. The reveal is valid in exactly one epoch — commit epoch + commit_reveal_period. Earlier fails with RevealTooEarly; later, the commit is expired and dropped."
    >
      <div className="grid grid-cols-4 gap-1.5 sm:grid-cols-8">
        {Array.from({length: TIMELINE_EPOCHS}, (_, epoch) => {
          const state = epochState(epoch, period);
          return (
            <div
              key={epoch}
              className={`border px-1 py-2 text-center ${EPOCH_STYLE[state]}`}
            >
              <p className="font-mono text-[0.6875rem]">e+{epoch}</p>
              <p className="mt-0.5 text-[0.625rem]">{EPOCH_LABEL[state]}</p>
            </div>
          );
        })}
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-3">
        <ExplainerStat
          label="Reveal epoch"
          value={`commit epoch + ${period}`}
          hint="valid in exactly this epoch"
        />
        <ExplainerStat
          label="Hidden for"
          value={`≈ ${delayBlocks} blocks`}
          hint={`≈ ${delayMinutes.toFixed(0)} min at tempo ${TEMPO_BLOCKS}, 12s blocks`}
        />
        <ExplainerStat
          label="Allowed range"
          value="1 – 100 epochs"
          hint="enforced by set_reveal_period"
        />
      </div>

      <div className="mt-5">
        <ExplainerSlider
          label="commit_reveal_period"
          value={period}
          min={1}
          max={TIMELINE_EPOCHS - 1}
          step={1}
          display={`${period} epoch${period === 1 ? '' : 's'}`}
          onChange={setPeriod}
        />
      </div>
    </ExplainerPanel>
  );
}

export function HyperparamWeightsRules({ focus }: { focus?: string }) {
  if (focus === 'commit_reveal_period' || focus === 'commit_reveal_weights_enabled') {
    return <CommitRevealTimeline />;
  }
  return <ClippingPlayground focus={focus} />;
}
