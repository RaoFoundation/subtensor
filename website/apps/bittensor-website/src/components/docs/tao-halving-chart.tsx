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
  Legend,
  type Plugin,
} from 'chart.js';
import { Line } from 'react-chartjs-2';
import { ExplainerPanel, ExplainerSlider, ExplainerStat } from './explainer-panel';
import {
  TOTAL_SUPPLY_TAO,
  blockEmissionTao,
  formatTao,
  halvingThresholdsTao,
} from '@/lib/emission-math';
import { DEFAULT_EMISSION_SNAPSHOT } from '@/lib/emission-snapshot';
import { ACCENT, AXIS_BORDER, GRAPH_FONT, GRID, INK, INK_FAINT, axisTitle, baseTicks } from './chart-theme';

ChartJS.register(CategoryScale, LinearScale, PointElement, LineElement, Filler, Tooltip, Legend);

const SAMPLE_POINTS = 120;
const THRESHOLDS = halvingThresholdsTao(6);
const LABELED_HALVINGS = 3;

function formatIssuanceM(value: number): string {
  const millions = value / 1_000_000;
  return `${millions.toFixed(Number.isInteger(millions * 10) ? 1 : 2)}M`;
}

export function TaoHalvingChart() {
  const [issuance, setIssuance] = useState(DEFAULT_EMISSION_SNAPSHOT.totalIssuanceTao);

  const chart = useMemo(() => {
    const xs = Array.from({length: SAMPLE_POINTS + 1}, (_, i) => (TOTAL_SUPPLY_TAO * i) / SAMPLE_POINTS);
    const ys = xs.map(blockEmissionTao);
    return {xs, ys};
  }, []);

  const currentEmission = blockEmissionTao(issuance);

  // The plugin is registered once at chart creation, so it reads live values
  // through a ref instead of closing over state that would go stale.
  const drawState = useRef({ issuance, currentEmission });
  drawState.current = { issuance, currentEmission };

  // Halving markers and direct series labels drawn in-plot: uppercase FiraCode
  // annotations instead of a legend. Red marks the halving thresholds only.
  const annotationPlugin = useMemo<Plugin<'line'>>(
    () => ({
      id: 'halvingAnnotations',
      afterDatasetsDraw(chart) {
        const { issuance, currentEmission } = drawState.current;
        const { ctx, chartArea, scales } = chart;
        const xScale = scales.x;
        const yScale = scales.y;
        if (!xScale || !yScale) return;

        ctx.save();
        ctx.font = GRAPH_FONT;

        // Halving thresholds: accent dots at each drop point; only the first
        // few carry a dashed guide and label so the crowded right edge stays quiet.
        THRESHOLDS.forEach((threshold, k) => {
          const xPx = xScale.getPixelForValue(threshold);
          if (xPx < chartArea.left || xPx > chartArea.right) return;

          const stepLevel = 1 / 2 ** k; // emission on the step ending here
          const levelAfter = stepLevel / 2;

          if (k < LABELED_HALVINGS) {
            ctx.strokeStyle = ACCENT;
            ctx.lineWidth = 1;
            ctx.setLineDash([3, 3]);
            ctx.beginPath();
            ctx.moveTo(xPx, chartArea.top);
            ctx.lineTo(xPx, chartArea.bottom);
            ctx.stroke();
            ctx.setLineDash([]);

            const yPx = Math.max(yScale.getPixelForValue(stepLevel) - 8, chartArea.top + 10);
            ctx.fillStyle = ACCENT;
            ctx.textAlign = 'right';
            ctx.fillText(`HALVING ${k + 1} · ${formatIssuanceM(threshold)}`, xPx - 6, yPx);
          }

          ctx.fillStyle = ACCENT;
          ctx.beginPath();
          ctx.arc(xPx, yScale.getPixelForValue(levelAfter), 2.5, 0, Math.PI * 2);
          ctx.fill();
        });

        // Selected issuance: quiet dashed guide plus an ink dot on the curve.
        const xSel = xScale.getPixelForValue(issuance);
        const ySel = yScale.getPixelForValue(currentEmission);
        ctx.strokeStyle = INK_FAINT;
        ctx.lineWidth = 1;
        ctx.setLineDash([4, 4]);
        ctx.beginPath();
        ctx.moveTo(xSel, chartArea.top);
        ctx.lineTo(xSel, chartArea.bottom);
        ctx.stroke();
        ctx.setLineDash([]);

        ctx.fillStyle = INK;
        ctx.beginPath();
        ctx.arc(xSel, ySel, 3.5, 0, Math.PI * 2);
        ctx.fill();

        const flip = xSel > (chartArea.left + chartArea.right) / 2;
        ctx.textAlign = flip ? 'right' : 'left';
        ctx.fillText('SELECTED ISSUANCE', xSel + (flip ? -8 : 8), chartArea.bottom - 8);

        // Direct series label instead of a legend.
        ctx.fillStyle = INK;
        ctx.textAlign = 'left';
        ctx.fillText('BLOCK EMISSION', chartArea.left + 6, Math.max(yScale.getPixelForValue(1) - 8, chartArea.top + 10));

        ctx.restore();
      },
    }),
    [],
  );

  const data = useMemo(
    () => ({
      datasets: [
        {
          label: 'Block emission (τ)',
          data: chart.xs.map((x, i) => ({x, y: chart.ys[i]})),
          borderColor: INK,
          backgroundColor: 'rgba(41, 41, 41, 0.03)',
          fill: true,
          tension: 0,
          pointRadius: 0,
          borderWidth: 1.5,
        },
      ],
    }),
    [chart],
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
            title: (items: {parsed: {x: number}}[]) =>
              `Issuance ${formatTao(items[0]?.parsed.x ?? 0, 2)}`,
            label: (ctx: {parsed: {y: number}}) => `${ctx.parsed.y.toFixed(4)} τ / block`,
          },
        },
      },
      scales: {
        x: {
          type: 'linear' as const,
          min: 0,
          max: TOTAL_SUPPLY_TAO,
          grid: {color: GRID},
          border: {color: AXIS_BORDER},
          ticks: baseTicks({
            callback: (value: string | number) => `${(Number(value) / 1_000_000).toFixed(1)}M`,
          }),
          title: axisTitle('total issuance (τ)'),
        },
        y: {
          min: 0,
          grid: {color: GRID},
          border: {color: AXIS_BORDER},
          ticks: baseTicks({maxTicksLimit: 5}),
          title: axisTitle('τ / block'),
        },
      },
    }),
    [],
  );

  const nextThreshold = THRESHOLDS.find((t) => t > issuance);

  return (
    <ExplainerPanel
      title="TAO halving curve"
      caption={
        <>
          Matches{' '}
          <a
            href="/code/pallets/subtensor/src/coinbase/block_emission.rs#L38-L81"
            className="underline"
          >
            get_block_emission_for_issuance
          </a>
          . Finney issuance today ≈ {formatTao(DEFAULT_EMISSION_SNAPSHOT.totalIssuanceTao, 2)}{' '}
          → {formatTao(DEFAULT_EMISSION_SNAPSHOT.blockEmissionTao)}/block.
        </>
      }
    >
      <div className="h-52">
        <Line data={data} options={options} plugins={[annotationPlugin]} />
      </div>

      <div className="mt-6 grid grid-cols-2 gap-x-8 gap-y-4 border-t border-line pt-4 sm:grid-cols-3">
        <ExplainerStat label="At selected issuance" value={formatTao(currentEmission) + ' / block'} />
        <ExplainerStat
          label="Daily at 12s blocks"
          value={formatTao(currentEmission * 7200, 2)}
          hint="7,200 blocks per day"
        />
        <ExplainerStat
          label="Next halving near"
          value={nextThreshold ? formatTao(nextThreshold, 2) + ' issued' : 'Cap reached'}
        />
      </div>

      <div className="mt-6 border-t border-line pt-4">
        <ExplainerSlider
          label="Total issuance"
          value={issuance}
          min={0}
          max={21_000_000}
          step={100_000}
          display={formatTao(issuance, 2)}
          onChange={setIssuance}
        />
      </div>
    </ExplainerPanel>
  );
}
