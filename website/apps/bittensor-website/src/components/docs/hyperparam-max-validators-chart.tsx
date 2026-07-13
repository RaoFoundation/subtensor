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

// Illustrative stake-weight distribution (α), already sorted descending —
// a typical long tail with a couple of zero-stake registrations at the end.
const STAKES = [
  9_400, 7_800, 6_500, 5_200, 4_100, 3_300, 2_700, 2_100, 1_600, 1_200, 900, 650, 420, 260, 140,
  60, 0, 0,
];

export function HyperparamMaxValidatorsChart() {
  const [maxValidators, setMaxValidators] = useState(8);

  // run_epoch.rs: is_topk_nonzero(&stake, max_allowed_validators) — top-k by
  // stake weight, but zero-stake neurons never receive a permit.
  const permitted = useMemo(
    () => STAKES.map((stake, i) => i < maxValidators && stake > 0),
    [maxValidators],
  );
  const permitCount = permitted.filter(Boolean).length;
  const lastPermitted = STAKES.filter((_, i) => permitted[i]).at(-1);

  const data = useMemo(
    () => ({
      labels: STAKES.map((_, i) => `#${i + 1}`),
      datasets: [
        {
          label: 'Stake weight (α)',
          data: [...STAKES],
          backgroundColor: permitted.map((hasPermit) =>
            hasPermit ? 'rgba(41, 41, 41, 0.65)' : 'rgba(41, 41, 41, 0.12)',
          ),
          borderColor: 'rgb(41, 41, 41)',
          borderWidth: 1,
        },
      ],
    }),
    [permitted],
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
              return `Neuron ranked #${idx + 1} by stake`;
            },
            label: (ctx: {parsed: {y: number}; dataIndex: number}) => {
              const status = permitted[ctx.dataIndex]
                ? 'validator permit'
                : STAKES[ctx.dataIndex] === 0
                  ? 'no permit (zero stake)'
                  : 'no permit — weights ignored';
              return `${ctx.parsed.y.toLocaleString()} α — ${status}`;
            },
          },
        },
      },
      scales: {
        x: {
          grid: {display: false},
          ticks: {font: {family: 'FiraCode, monospace', size: 10}},
          title: {display: true, text: 'neurons, sorted by stake weight', font: {size: 11}},
        },
        y: {
          min: 0,
          grid: {color: 'rgba(41, 41, 41, 0.06)'},
          ticks: {font: {family: 'FiraCode, monospace', size: 10}},
          title: {display: true, text: 'stake weight (α)', font: {size: 11}},
        },
      },
    }),
    [permitted],
  );

  return (
    <ExplainerPanel
      title="max_validators permit line"
      caption="Neurons sorted by stake weight; every epoch is_topk_nonzero (run_epoch.rs) grants permits to the top max_validators non-zero-stake neurons. Solid bars hold a permit; faded bars past the line have their weights discarded and stake masked from consensus. Slide the cap to move the line."
    >
      <div className="h-52">
        <Bar data={data} options={options} />
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-3">
        <ExplainerStat
          label="Permits granted"
          value={`${permitCount} / ${STAKES.length}`}
          hint="zero-stake neurons never qualify"
        />
        <ExplainerStat
          label="Lowest permitted stake"
          value={lastPermitted !== undefined ? `${lastPermitted.toLocaleString()} α` : '—'}
          hint="the effective bar to validate"
        />
        <ExplainerStat label="Mainnet default" value="128" hint="≤ max_allowed_uids" />
      </div>

      <div className="mt-5">
        <ExplainerSlider
          label="max_validators (permit cap)"
          value={maxValidators}
          min={1}
          max={STAKES.length}
          step={1}
          display={String(maxValidators)}
          onChange={setMaxValidators}
        />
      </div>
    </ExplainerPanel>
  );
}
