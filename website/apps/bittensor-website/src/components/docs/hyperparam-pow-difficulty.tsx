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

// Per-focus starting scenario: which bound the simulated difficulty walk rides,
// and what the panel caption should emphasize.
const SCENARIOS: Record<
  string,
  { floorLog: number; ceilLog: number; regs: number; target: number; caption: string }
> = {
  difficulty: {
    floorLog: 7,
    ceilLog: 16,
    regs: 6,
    target: 2,
    caption:
      'difficulty is a direct price in compute: hash_meets_difficulty passes with probability about 1 in difficulty, so a nonce costs ~difficulty hash attempts on average. The classic controller rescaled it by (regs + target) / (2 \u00d7 target) each adjustment interval, clamped to [min_difficulty, max_difficulty]. On the current runtime the register extrinsic routes to burned registration, so this controller no longer runs.',
  },
  min_difficulty: {
    floorLog: 7,
    ceilLog: 16,
    regs: 0,
    target: 2,
    caption:
      'This scenario starts with quiet demand (0 registrations vs a target of 2), so the controller decays difficulty every interval \u2014 until it lands on the bold min_difficulty floor and sits there. The floor is the one price the controller could never undercut. Drag the floor to its top stop (or use the button) for the u64::MAX disabled state.',
  },
  max_difficulty: {
    floorLog: 5,
    ceilLog: 10,
    regs: 12,
    target: 2,
    caption:
      'This scenario starts with a registration rush (12 registrations vs a target of 2), so the controller ratchets difficulty up every interval \u2014 until it hits the bold max_difficulty ceiling and pins there. The ceiling capped how expensive a PoW slot could get during a rush. The mainnet default is u64::MAX / 4.',
  },
};

export function HyperparamPowDifficulty({ focus }: { focus?: string }) {
  const scenario = (focus && SCENARIOS[focus]) || SCENARIOS['difficulty'];

  const [floorLog, setFloorLog] = useState(scenario.floorLog);
  const [ceilLog, setCeilLog] = useState(scenario.ceilLog);
  const [regsPerInterval, setRegsPerInterval] = useState(scenario.regs);
  const [targetRegs, setTargetRegs] = useState(scenario.target);

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

  const floorFocused = focus === 'min_difficulty';
  const ceilFocused = focus === 'max_difficulty';

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
          label: floorFocused ? 'min_difficulty (floor) \u2190 this page' : 'min_difficulty (floor)',
          data: series.map(() => floor),
          borderColor: floorFocused ? 'rgb(41, 41, 41)' : 'rgba(41, 41, 41, 0.35)',
          borderDash: floorFocused ? [6, 3] : [4, 4],
          fill: false,
          pointRadius: 0,
          borderWidth: floorFocused ? 2.5 : 1,
        },
        {
          label: ceilFocused ? 'max_difficulty (ceiling) \u2190 this page' : 'max_difficulty (ceiling)',
          data: series.map(() => ceiling),
          borderColor: ceilFocused ? 'rgb(41, 41, 41)' : 'rgba(41, 41, 41, 0.35)',
          borderDash: ceilFocused ? [6, 3] : [2, 3],
          fill: false,
          pointRadius: 0,
          borderWidth: ceilFocused ? 2.5 : 1,
        },
      ],
    }),
    [series, floor, ceiling, floorFocused, ceilFocused],
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
    <ExplainerPanel title="PoW difficulty controller" caption={scenario.caption}>
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
          label={mark('difficulty', 'Odds a single nonce passes')}
          value={powDisabled ? '0 (no nonce can pass)' : `\u2248 1 in ${formatDifficulty(finalDifficulty)}`}
          hint={
            powDisabled
              ? 'at u64::MAX the seal check rejects everything'
              : `hash_meets_difficulty \u2192 expect ~${formatDifficulty(finalDifficulty)} hashes per registration`
          }
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

      <div className="mt-4 flex items-center gap-3">
        {powDisabled ? (
          <button
            type="button"
            onClick={() => setFloorLog(scenario.floorLog)}
            className="border border-line bg-panel px-3 py-1 font-mono text-[0.75rem] hover:bg-bg"
          >
            re-enable PoW (restore floor)
          </button>
        ) : (
          <button
            type="button"
            onClick={() => setFloorLog(DISABLED_LOG)}
            className="border border-line bg-panel px-3 py-1 font-mono text-[0.75rem] hover:bg-bg"
          >
            set floor to u64::MAX (disable PoW)
          </button>
        )}
        <span className="text-[0.7rem] text-mute">
          {powDisabled
            ? 'difficulty is pinned at u64::MAX: no nonce can ever pass the seal check.'
            : 'the sentinel state: a floor of u64::MAX pins difficulty at maximum.'}
        </span>
      </div>
    </ExplainerPanel>
  );
}
