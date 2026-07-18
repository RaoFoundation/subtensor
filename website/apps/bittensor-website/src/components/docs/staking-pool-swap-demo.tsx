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
import { ExplainerPanel, ExplainerSlider, ExplainerStat, ExplainerToggle } from './explainer-panel';
import {
  ACCENT,
  AXIS_BORDER,
  GRAPH_FONT,
  GRID,
  INK,
  INK_FAINT,
  axisTitle,
  baseTicks,
} from './chart-theme';

ChartJS.register(CategoryScale, LinearScale, PointElement, LineElement, Filler, Tooltip, Legend);

// Default FeeRate = 33, normalized over u16::MAX — ≈0.05% taken from the
// input side before the swap (swap_step.rs, paid to the block author).
const FEE_RATE = 33 / 65535;
const SPOT_PRICE = 0.05; // TAO per alpha at any slider setting; reserves derive from it
const CURVE_SAMPLES = 60;
const X_MAX = 10_000;

type Direction = 'stake' | 'unstake';

// p = (w1·y) / (w2·x), so fixing p and y determines the alpha reserve.
function alphaReserveFor(taoReserve: number, quoteWeight: number): number {
  const baseWeight = 1 - quoteWeight;
  return (baseWeight * taoReserve) / (quoteWeight * SPOT_PRICE);
}

// Buy alpha with ∆y TAO: ∆x = x · (1 − (y/(y+∆y))^(w2/w1))
function alphaOutForTaoIn(x: number, y: number, quoteWeight: number, taoIn: number): number {
  const baseWeight = 1 - quoteWeight;
  return x * (1 - Math.pow(y / (y + taoIn), quoteWeight / baseWeight));
}

// Sell ∆x alpha: ∆y = y · (1 − (x/(x+∆x))^(w1/w2))
function taoOutForAlphaIn(x: number, y: number, quoteWeight: number, alphaIn: number): number {
  const baseWeight = 1 - quoteWeight;
  return y * (1 - Math.pow(x / (x + alphaIn), baseWeight / quoteWeight));
}

// Effective price (TAO per alpha) for a gross input, fee taken from the input side.
function effectivePrice(
  x: number,
  y: number,
  quoteWeight: number,
  direction: Direction,
  grossIn: number,
): number {
  const netIn = grossIn * (1 - FEE_RATE);
  if (direction === 'stake') {
    return grossIn / alphaOutForTaoIn(x, y, quoteWeight, netIn);
  }
  return taoOutForAlphaIn(x, y, quoteWeight, netIn) / grossIn;
}

function formatTao(value: number): string {
  return `${value.toLocaleString('en-US', { maximumFractionDigits: 0 })} τ`;
}

function formatPrice(value: number): string {
  return `${value.toFixed(6)} τ/α`;
}

export function StakingPoolSwapDemo() {
  const [taoReserve, setTaoReserve] = useState(50_000);
  const [tradeSize, setTradeSize] = useState(2_000);
  const [quoteWeight, setQuoteWeight] = useState(0.5);
  const [direction, setDirection] = useState<Direction>('stake');

  const alphaReserve = alphaReserveFor(taoReserve, quoteWeight);
  const baseWeight = 1 - quoteWeight;
  // Recomputed from reserves as the pallet does; equals SPOT_PRICE by construction.
  const spot = (baseWeight * taoReserve) / (quoteWeight * alphaReserve);

  // For unstake the slider value is interpreted as alpha sold, so the x axis
  // stays a plain "input tokens" scale in both directions.
  const fee = tradeSize * FEE_RATE;
  const netIn = tradeSize - fee;
  const output =
    direction === 'stake'
      ? alphaOutForTaoIn(alphaReserve, taoReserve, quoteWeight, netIn)
      : taoOutForAlphaIn(alphaReserve, taoReserve, quoteWeight, netIn);
  const execPrice = effectivePrice(alphaReserve, taoReserve, quoteWeight, direction, tradeSize);
  const impactPct = ((direction === 'stake' ? execPrice - spot : spot - execPrice) / spot) * 100;

  // The plugin is registered once at chart creation, so it reads live values
  // through a ref instead of closing over state that would go stale.
  const drawState = useRef({ alphaReserve, taoReserve, quoteWeight, direction, spot });
  drawState.current = { alphaReserve, taoReserve, quoteWeight, direction, spot };

  // Direct in-plot curve labels (uppercase FiraCode) instead of a legend.
  const annotationPlugin = useMemo<Plugin<'line'>>(
    () => ({
      id: 'swapCurveLabels',
      afterDatasetsDraw(chart) {
        const { alphaReserve, taoReserve, quoteWeight, direction, spot } = drawState.current;
        const { ctx, scales } = chart;
        const xScale = scales.x;
        const yScale = scales.y;
        if (!xScale || !yScale) return;

        // Label at ~65% of the x-range, where the curves have separated. The
        // spot line hugs the chart edge (bottom for stake, top for unstake),
        // so its label goes on the inner side; the execution label goes on the
        // side the sloped curve moves away from, so the text isn't crossed.
        const xLabel = X_MAX * 0.65;
        const xPx = xScale.getPixelForValue(xLabel);
        const execY = yScale.getPixelForValue(
          effectivePrice(alphaReserve, taoReserve, quoteWeight, direction, xLabel),
        );
        const spotY = yScale.getPixelForValue(spot);

        ctx.save();
        ctx.font = GRAPH_FONT;
        ctx.textAlign = 'left';
        ctx.fillStyle = INK;
        ctx.fillText('EXECUTION PRICE', xPx + 4, direction === 'stake' ? execY + 16 : execY - 8);
        ctx.fillStyle = INK_FAINT;
        ctx.fillText('SPOT PRICE', xPx + 4, direction === 'stake' ? spotY - 8 : spotY + 16);
        ctx.restore();
      },
    }),
    [],
  );

  const curve = useMemo(() => {
    const points: { x: number; y: number }[] = [];
    for (let i = 1; i <= CURVE_SAMPLES; i++) {
      const size = (X_MAX * i) / CURVE_SAMPLES;
      points.push({
        x: size,
        y: effectivePrice(alphaReserve, taoReserve, quoteWeight, direction, size),
      });
    }
    return points;
  }, [alphaReserve, taoReserve, quoteWeight, direction]);

  const spotLine = useMemo(
    () => [
      { x: 0, y: spot },
      { x: X_MAX, y: spot },
    ],
    [spot],
  );

  const data = useMemo(
    () => ({
      datasets: [
        {
          label: 'Effective execution price',
          data: curve,
          borderColor: INK,
          backgroundColor: 'rgba(41, 41, 41, 0.03)',
          fill: 'origin' as const,
          tension: 0.2,
          pointRadius: 0,
          borderWidth: 1.75,
        },
        {
          label: 'Spot price',
          data: spotLine,
          borderColor: 'rgba(41, 41, 41, 0.5)',
          borderDash: [4, 4],
          pointRadius: 0,
          borderWidth: 1,
        },
        {
          label: 'Your trade',
          data: [{ x: tradeSize, y: execPrice }],
          borderColor: ACCENT,
          backgroundColor: ACCENT,
          showLine: false,
          pointRadius: 4,
          pointStyle: 'circle' as const,
        },
      ],
    }),
    [curve, spotLine, tradeSize, execPrice],
  );

  const inputUnit = direction === 'stake' ? 'τ' : 'α';

  const options = useMemo(
    () => ({
      responsive: true,
      maintainAspectRatio: false,
      interaction: { mode: 'nearest' as const, intersect: false },
      plugins: {
        legend: { display: false },
        tooltip: {
          callbacks: {
            title: (items: { parsed: { x: number } }[]) =>
              `trade ${(items[0]?.parsed.x ?? 0).toLocaleString('en-US', { maximumFractionDigits: 0 })} ${inputUnit}`,
            label: (ctx: { parsed: { y: number }; dataset: { label?: string } }) =>
              `${ctx.dataset.label ?? ''}: ${ctx.parsed.y.toFixed(6)} τ/α`,
          },
        },
      },
      scales: {
        x: {
          type: 'linear' as const,
          min: 0,
          max: X_MAX,
          grid: { color: GRID },
          border: { color: AXIS_BORDER },
          ticks: baseTicks({
            callback: (value: string | number) => Number(value).toLocaleString('en-US'),
          }),
          title: axisTitle(direction === 'stake' ? 'trade size (TAO in)' : 'trade size (alpha in)'),
        },
        y: {
          grid: { color: GRID },
          border: { color: AXIS_BORDER },
          ticks: baseTicks({
            maxTicksLimit: 5,
            callback: (value: string | number) => Number(value).toFixed(4),
          }),
          title: axisTitle('execution price (τ per α)'),
        },
      },
    }),
    [direction, inputUnit],
  );

  return (
    <ExplainerPanel
      title="Staking pool swap"
      tag="balancer pool"
      caption={
        <>
          Each subnet&apos;s stake operations swap through a weighted pool of TAO and alpha (
          <a href="/code/pallets/swap/src/pallet/balancer.rs#L9-L22" className="underline">
            pallets/swap/src/pallet/balancer.rs
          </a>
          ): buying alpha pays ∆x = x·(1 − (y/(y+∆y))^(w2/w1)), so the effective price drifts
          away from spot p = (w1·y)/(w2·x) as trade size grows. A ≈0.05% fee (FeeRate 33/65535)
          is taken from the input side before the swap and paid to the block author. Alpha
          reserve is derived so spot is 0.05 τ/α.
        </>
      }
    >
      {/* Chart */}
      <div className="h-72">
        <Line data={data} options={options} plugins={[annotationPlugin]} />
      </div>

      {/* Result summary */}
      <div className="mt-8 border-t border-line pt-4">
        <div className="grid grid-cols-2 gap-x-8 gap-y-4 sm:grid-cols-4">
          <ExplainerStat
            label="Spot price"
            value={formatPrice(spot)}
            hint={`p = (w1·y)/(w2·x), w2 = ${quoteWeight.toFixed(2)}`}
          />
          <ExplainerStat
            label={direction === 'stake' ? 'Alpha received' : 'TAO received'}
            value={
              direction === 'stake'
                ? `${output.toLocaleString('en-US', { maximumFractionDigits: 2 })} α`
                : `${output.toLocaleString('en-US', { maximumFractionDigits: 2 })} τ`
            }
            hint={`after ${fee.toLocaleString('en-US', { maximumFractionDigits: 2 })} ${inputUnit} fee (≈0.05% of input)`}
          />
          <ExplainerStat
            label="Effective price"
            value={formatPrice(execPrice)}
            hint={direction === 'stake' ? 'TAO paid per alpha received' : 'TAO received per alpha sold'}
          />
          <ExplainerStat
            label="Price impact"
            value={`${impactPct.toFixed(3)}%`}
            hint={direction === 'stake' ? 'paying above spot' : 'receiving below spot'}
          />
        </div>
      </div>

      {/* Controls */}
      <div className="mt-8 border-t border-line pt-4 pb-1">
        <ExplainerToggle
          label="direction"
          options={[
            { id: 'stake', label: 'stake (buy α)' },
            { id: 'unstake', label: 'unstake (sell α)' },
          ]}
          value={direction}
          onChange={setDirection}
        />
        <div className="mt-6 grid gap-x-8 gap-y-5 sm:grid-cols-3">
          <ExplainerSlider
            label="pool TAO reserve (y)"
            value={taoReserve}
            min={1_000}
            max={500_000}
            step={1_000}
            display={formatTao(taoReserve)}
            onChange={setTaoReserve}
          />
          <ExplainerSlider
            label={direction === 'stake' ? 'stake size (∆y TAO in)' : 'unstake size (∆x alpha in)'}
            value={tradeSize}
            min={1}
            max={10_000}
            step={1}
            display={`${tradeSize.toLocaleString('en-US')} ${inputUnit}`}
            onChange={setTradeSize}
          />
          <ExplainerSlider
            label="quote weight (w2)"
            value={quoteWeight}
            min={0.3}
            max={0.7}
            step={0.01}
            display={`w2 = ${quoteWeight.toFixed(2)}, w1 = ${baseWeight.toFixed(2)}`}
            onChange={setQuoteWeight}
          />
        </div>
      </div>
    </ExplainerPanel>
  );
}
