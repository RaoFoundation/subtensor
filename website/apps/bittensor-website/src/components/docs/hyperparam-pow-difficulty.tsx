'use client';

import { useMemo, useState } from 'react';
import {
  Chart as ChartJS,
  CategoryScale,
  LogarithmicScale,
  PointElement,
  LineElement,
  Tooltip,
} from 'chart.js';
import { Line } from 'react-chartjs-2';
import { ExplainerPanel, ExplainerSlider, ExplainerStat } from './explainer-panel';

ChartJS.register(CategoryScale, LogarithmicScale, PointElement, LineElement, Tooltip);

const U64_MAX = 2 ** 64;
// Slider positions are log10(difficulty); the top position stands for u64::MAX.
const DISABLED_LOG = 20;
const INTERVALS = 48;
const START_DIFFICULTY = 10_000_000; // chain default (InitialDifficulty)

function fromLog(log: number): number {
  return log >= DISABLED_LOG ? U64_MAX : 10 ** log;
}

function formatDifficulty(value: number): string {
  if (value >= U64_MAX) return 'u64::MAX';
  if (value >= 1_000_000) return value.toExponential(1).replace('e+', 'e');
  return Math.round(value).toLocaleString();
}

function clamp(value: number, lo: number, hi: number): number {
  return Math.min(Math.max(value, lo), hi);
}

export function HyperparamPowDifficulty({ focus }: { focus?: string }) {
  const [floorLog, setFloorLog] = useState(7); // min_difficulty = 1e7 (chain default)
  const [ceilLog, setCeilLog] = useState(16);
  const [regsPerInterval, setRegsPerInterval] = useState(6);
  const [targetRegs, setTargetRegs] = useState(2);

  const floor = fromLog(floorLog);
  const ceiling = Math.max(fromLog(ceilLog), floor);
  const powDisabled = floor >= U64_MAX;

  const mark = (param: string, label: string) => (focus === param ? `${label} \u2190 this page` : label);

  const series = useMemo(() => {
    const points: number[] = [];
    let difficulty = clamp(START_DIFFICULTY, floor, ceiling);
    for (let i = 0; i <= INTERVALS; i++) {
      points.push(difficulty);
      // Classic controller step: difficulty *= (regs + target) / (2 * target),
      // clamped to [min_difficulty, max_difficulty].
      const ratio = (regsPerInterval + targetRegs) / (2 * targetRegs);
      difficulty = clamp(difficulty * ratio, floor, ceiling);
    }
    return points;
  }, [floor, ceiling, regsPerInterval, targetRegs]);

  const finalDifficulty = series[series.length - 1];

  const trend = powDisabled
    ? 'PoW disabled'
    : regsPerInterval > targetRegs
      ? finalDifficulty >= ceiling
        ? 'pinned at ceiling'
        : 'rising toward ceiling'
      : regsPerInterval < targetRegs
        ? finalDifficulty <= floor
          ? 'pinned at floor'
          : 'falling toward floor'
        : 'steady at target';

  const data = useMemo(
    () => ({
      labels: series.map((_, i) => `${i}`),
      datasets: [
        {
          label: 'difficulty',
          data: series,
          borderColor: 'rgb(41, 41, 41)',
          backgroundColor: 'rgba(41, 41, 41, 0.08)',
          fill: false,
          tension: 0,
          pointRadius: 0,
          borderWidth: 1.5,
        },
        {
          label: 'min_difficulty (floor)',
          data: series.map(() => floor),
          borderColor: 'rgba(41, 41, 41, 0.35)',
          borderDash: [4, 4],
          fill: false,
          pointRadius: 0,
          borderWidth: 1,
        },
        {
          label: 'max_difficulty (ceiling)',
          data: series.map(() => ceiling),
          borderColor: 'rgba(41, 41, 41, 0.35)',
          borderDash: [2, 3],
          fill: false,
          pointRadius: 0,
          borderWidth: 1,
        },
      ],
    }),
    [series, floor, ceiling],
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
            title: (items: { dataIndex: number }[]) => `Interval ${items[0]?.dataIndex ?? 0}`,
            label: (ctx: { dataset: { label?: string }; parsed: { y: number } }) =>
              `${ctx.dataset.label ?? ''}: ${formatDifficulty(ctx.parsed.y)}`,
          },
        },
      },
      scales: {
        x: {
          grid: { color: 'rgba(41, 41, 41, 0.06)' },
          ticks: { maxTicksLimit: 8, font: { family: 'FiraCode, monospace', size: 10 } },
          title: { display: true, text: 'adjustment intervals', font: { size: 11 } },
        },
        y: {
          type: 'logarithmic' as const,
          grid: { color: 'rgba(41, 41, 41, 0.06)' },
          ticks: {
            font: { family: 'FiraCode, monospace', size: 10 },
            callback: (value: string | number) => formatDifficulty(Number(value)),
            maxTicksLimit: 6,
          },
        },
      },
    }),
    [],
  );

  return (
    <ExplainerPanel
      title="PoW difficulty controller"
      caption="The classic per-interval adjustment: difficulty scales by (regs + target) / (2 x target) each adjustment interval, clamped to [min_difficulty, max_difficulty]. Drag the floor to its top stop to see the u64::MAX state. On the current runtime the register extrinsic routes to burned registration, so this controller no longer runs."
    >
      <div className="h-52">
        <Line data={data} options={options} />
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-3">
        <ExplainerStat
          label={mark('difficulty', 'Difficulty after 48 intervals')}
          value={powDisabled ? 'u64::MAX' : formatDifficulty(finalDifficulty)}
          hint={powDisabled ? 'PoW registration effectively disabled' : trend}
        />
        <ExplainerStat
          label="Expected hashes per registration"
          value={powDisabled ? '\u221e (no nonce can pass)' : `\u2248 ${formatDifficulty(finalDifficulty)}`}
          hint="hash_meets_difficulty passes ~1 in difficulty attempts"
        />
        <ExplainerStat
          label="Registration pressure"
          value={`${regsPerInterval} regs vs target ${targetRegs}`}
          hint={
            regsPerInterval > targetRegs
              ? 'over target: difficulty ratchets up'
              : regsPerInterval < targetRegs
                ? 'under target: difficulty decays down'
                : 'on target: difficulty holds'
          }
        />
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-2">
        <ExplainerSlider
          label={mark('min_difficulty', 'min_difficulty (floor)')}
          value={floorLog}
          min={3}
          max={DISABLED_LOG}
          step={0.5}
          display={powDisabled ? 'u64::MAX = PoW disabled' : formatDifficulty(floor)}
          onChange={setFloorLog}
        />
        <ExplainerSlider
          label={mark('max_difficulty', 'max_difficulty (ceiling)')}
          value={ceilLog}
          min={4}
          max={DISABLED_LOG}
          step={0.5}
          display={ceiling >= U64_MAX ? 'u64::MAX = unbounded' : formatDifficulty(ceiling)}
          onChange={setCeilLog}
        />
        <ExplainerSlider
          label="Registrations per interval (demand)"
          value={regsPerInterval}
          min={0}
          max={20}
          step={1}
          display={`${regsPerInterval}`}
          onChange={setRegsPerInterval}
        />
        <ExplainerSlider
          label="target_regs_per_interval"
          value={targetRegs}
          min={1}
          max={10}
          step={1}
          display={`${targetRegs}`}
          onChange={setTargetRegs}
        />
      </div>
    </ExplainerPanel>
  );
}
