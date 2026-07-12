'use client';

import { useMemo, useState } from 'react';
import {
  Chart as ChartJS,
  CategoryScale,
  LinearScale,
  PointElement,
  LineElement,
  Tooltip,
  Legend,
} from 'chart.js';
import { Line } from 'react-chartjs-2';
import { ExplainerPanel, ExplainerSlider, ExplainerStat } from './explainer-panel';
import {
  DEFAULT_BURN_HALF_LIFE,
  DEFAULT_BURN_INCREASE_MULT,
  DEFAULT_INITIAL_BURN_TAO,
  DEFAULT_MAX_BURN_TAO,
  DEFAULT_MIN_BURN_TAO,
  formatTao,
  simulateBurnPrice,
} from '@/lib/emission-math';

ChartJS.register(CategoryScale, LinearScale, PointElement, LineElement, Tooltip, Legend);

export function RegistrationBurnTimeline() {
  const [blocks, setBlocks] = useState(720);
  const [regBlock, setRegBlock] = useState(360);
  const [halfLife, setHalfLife] = useState(DEFAULT_BURN_HALF_LIFE);

  const prices = useMemo(
    () =>
      simulateBurnPrice(
        blocks,
        [regBlock],
        halfLife,
        DEFAULT_BURN_INCREASE_MULT,
        DEFAULT_MIN_BURN_TAO,
        DEFAULT_MAX_BURN_TAO,
        DEFAULT_INITIAL_BURN_TAO,
      ),
    [blocks, regBlock, halfLife],
  );

  const data = useMemo(
    () => ({
      labels: prices.map((_, i) => String(i)),
      datasets: [
        {
          label: 'Registration burn',
          data: prices,
          borderColor: 'rgb(41, 41, 41)',
          pointRadius: prices.map((_, i) => (i === regBlock ? 4 : 0)),
          pointBackgroundColor: 'rgb(41, 41, 41)',
          borderWidth: 1.5,
          tension: 0.1,
        },
      ],
    }),
    [prices, regBlock],
  );

  const options = useMemo(
    () => ({
      responsive: true,
      maintainAspectRatio: false,
      plugins: {
        legend: {display: false},
        tooltip: {
          callbacks: {
            label: (ctx: {parsed: {y: number}}) => formatTao(ctx.parsed.y),
          },
        },
      },
      scales: {
        x: {
          display: true,
          title: {display: true, text: 'Block', font: {size: 11}},
          ticks: {maxTicksLimit: 8, font: {size: 10}},
        },
        y: {
          grid: {color: 'rgba(41, 41, 41, 0.06)'},
          ticks: {
            callback: (v: number | string) => formatTao(Number(v)),
            font: {size: 10},
          },
        },
      },
    }),
    [],
  );

  const atReg = prices[regBlock] ?? 0;
  const afterReg = prices[Math.min(regBlock + 1, prices.length - 1)] ?? 0;

  return (
    <ExplainerPanel
      title="Registration burn price"
      caption="Price decays continuously (halving every BurnHalfLife blocks) and jumps ×1.26 on each successful registration."
    >
      <div className="h-44">
        <Line data={data} options={options} />
      </div>

      <div className="mt-4 grid gap-3 sm:grid-cols-3">
        <ExplainerStat label="At registration block" value={formatTao(atReg)} />
        <ExplainerStat
          label="After ×1.26 bump"
          value={formatTao(Math.min(DEFAULT_MAX_BURN_TAO, atReg * DEFAULT_BURN_INCREASE_MULT))}
        />
        <ExplainerStat label="Decay per half-life" value="÷ 2" hint={`Every ${halfLife} blocks`} />
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-2">
        <ExplainerSlider
          label="Simulated window (blocks)"
          value={blocks}
          min={180}
          max={2_000}
          step={60}
          display={`${blocks} blocks`}
          onChange={setBlocks}
        />
        <ExplainerSlider
          label="Registration event at block"
          value={regBlock}
          min={30}
          max={Math.max(60, blocks - 30)}
          step={30}
          display={`block ${regBlock}`}
          onChange={setRegBlock}
        />
        <ExplainerSlider
          label="BurnHalfLife"
          value={halfLife}
          min={120}
          max={720}
          step={60}
          display={`${halfLife} blocks (~${((halfLife * 12) / 3600).toFixed(1)} h)`}
          onChange={setHalfLife}
        />
        <ExplainerStat
          label="Price after event (next block)"
          value={formatTao(afterReg)}
          hint="Clamped to subnet MinBurn / MaxBurn"
        />
      </div>
    </ExplainerPanel>
  );
}
