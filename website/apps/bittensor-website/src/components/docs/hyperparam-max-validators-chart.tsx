'use client';

import { useMemo, useRef, useState } from 'react';
import {
  Chart as ChartJS,
  CategoryScale,
  LinearScale,
  BarElement,
  Tooltip,
  Legend,
  type Plugin,
} from 'chart.js';
import { Bar } from 'react-chartjs-2';
import { ExplainerPanel, ExplainerSlider, ExplainerStat } from './explainer-panel';
import {
  ACCENT,
  ACCENT_REGION,
  AXIS_BORDER,
  GRAPH_FONT,
  GRID,
  INK,
  INK_FAINT,
  axisTitle,
  baseTicks,
} from './chart-theme';

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

  // The plugin is registered once at chart creation, so it reads live values
  // through a ref instead of closing over state that would go stale.
  const drawState = useRef({ maxValidators });
  drawState.current = { maxValidators };

  // Permit line drawn in-plot: blocked region tint past the cap, a dashed
  // boundary guide, and uppercase FiraCode labels (no legend).
  const annotationPlugin = useMemo<Plugin<'bar'>>(
    () => ({
      id: 'permitLineAnnotations',
      beforeDatasetsDraw(chart) {
        const { maxValidators } = drawState.current;
        const { ctx, chartArea, scales } = chart;
        const xScale = scales.x;
        if (!xScale || maxValidators >= STAKES.length) return;

        // Category scale: the boundary sits halfway between the last permitted
        // rank and the first excluded one.
        const xBoundary = xScale.getPixelForValue(maxValidators - 0.5);

        ctx.save();
        ctx.fillStyle = ACCENT_REGION;
        ctx.fillRect(xBoundary, chartArea.top, chartArea.right - xBoundary, chartArea.height);

        ctx.strokeStyle = INK_FAINT;
        ctx.lineWidth = 1;
        ctx.setLineDash([4, 4]);
        ctx.beginPath();
        ctx.moveTo(xBoundary, chartArea.top);
        ctx.lineTo(xBoundary, chartArea.bottom);
        ctx.stroke();
        ctx.restore();
      },
      afterDatasetsDraw(chart) {
        const { maxValidators } = drawState.current;
        const { ctx, chartArea, scales } = chart;
        const xScale = scales.x;
        if (!xScale || maxValidators >= STAKES.length) return;

        const xBoundary = xScale.getPixelForValue(maxValidators - 0.5);

        ctx.save();
        ctx.font = GRAPH_FONT;

        // High up beside the guide, clear of the long-tail bars at the bottom.
        ctx.fillStyle = INK;
        if (chartArea.right - xBoundary > 130) {
          ctx.textAlign = 'left';
          ctx.fillText(`MAX_VALIDATORS = ${maxValidators}`, xBoundary + 6, chartArea.top + 46);
        } else {
          ctx.textAlign = 'right';
          ctx.fillText(`MAX_VALIDATORS = ${maxValidators}`, xBoundary - 6, chartArea.top + 46);
        }

        if (chartArea.right - xBoundary > 120) {
          ctx.fillStyle = ACCENT;
          ctx.textAlign = 'center';
          const cx = (xBoundary + chartArea.right) / 2;
          ctx.fillText('NO PERMIT', cx, chartArea.top + 14);
          ctx.fillText('WEIGHTS DISCARDED', cx, chartArea.top + 28);
        }

        ctx.restore();
      },
    }),
    [],
  );

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
          borderColor: INK,
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
          border: {color: AXIS_BORDER},
          ticks: baseTicks(),
          title: axisTitle('neurons, sorted by stake weight'),
        },
        y: {
          min: 0,
          grid: {color: GRID},
          border: {color: AXIS_BORDER},
          ticks: baseTicks({maxTicksLimit: 5}),
          title: axisTitle('stake weight (α)'),
        },
      },
    }),
    [permitted],
  );

  return (
    <ExplainerPanel
      title="max_validators permit line"
      caption={
        <>
          Neurons sorted by stake weight; every epoch{' '}
          <a
            href="/code/pallets/subtensor/src/epoch/math.rs#L227-L241"
            className="underline"
          >
            is_topk_nonzero (epoch/math.rs)
          </a>{' '}
          grants permits to the top max_validators non-zero-stake neurons. Solid bars hold a
          permit; faded bars past the line have their weights discarded and stake masked from
          consensus. Slide the cap to move the line.
        </>
      }
    >
      <div className="h-52">
        <Bar data={data} options={options} plugins={[annotationPlugin]} />
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
