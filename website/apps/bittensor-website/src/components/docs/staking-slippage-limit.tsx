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
  ACCENT_REGION,
  AXIS_BORDER,
  GRAPH_FONT,
  GRID,
  INK,
  INK_FAINT,
  axisTitle,
  baseTicks,
} from './chart-theme';

ChartJS.register(CategoryScale, LinearScale, PointElement, LineElement, Filler, Tooltip, Legend);

const SPOT_PRICE = 0.05; // TAO per alpha; alpha reserve is derived from pool depth
const QUOTE_WEIGHT = 0.5; // w2; w1 = 1 − w2. Default weights, kept fixed here.
const BASE_WEIGHT = 1 - QUOTE_WEIGHT;
const MAX_TRADE = 20_000;
const CURVE_SAMPLES = 80;

// Buy alpha with ∆y TAO: ∆x = x · (1 − (y/(y+∆y))^(w2/w1))
function alphaOutForTaoIn(x: number, y: number, taoIn: number): number {
  return x * (1 - Math.pow(y / (y + taoIn), QUOTE_WEIGHT / BASE_WEIGHT));
}

// Average TAO paid per alpha received for a trade of the given size.
function executionPrice(x: number, y: number, taoIn: number): number {
  return taoIn / alphaOutForTaoIn(x, y, taoIn);
}

// Marginal pool price after the trade: p = (w1·y')/(w2·x')
function poolPriceAfter(x: number, y: number, taoIn: number): number {
  const alphaOut = alphaOutForTaoIn(x, y, taoIn);
  return (BASE_WEIGHT * (y + taoIn)) / (QUOTE_WEIGHT * (x - alphaOut));
}

function formatTao(value: number): string {
  return `${value.toLocaleString('en-US', { maximumFractionDigits: 0 })} τ`;
}

function formatPrice(value: number): string {
  return `${value.toFixed(6)} τ/α`;
}

export function StakingSlippageLimit() {
  const [tolerancePct, setTolerancePct] = useState(2);
  const [tradeSize, setTradeSize] = useState(1_200);
  const [poolDepth, setPoolDepth] = useState(100_000);
  const [allowPartial, setAllowPartial] = useState(true);

  const taoReserve = poolDepth;
  const alphaReserve = (BASE_WEIGHT * taoReserve) / (QUOTE_WEIGHT * SPOT_PRICE);
  const limitPrice = SPOT_PRICE * (1 + tolerancePct / 100);
  // Max TAO in before the pool price reaches the limit: ∆y = y·((p'/p)^w1 − 1)
  const maxFill = taoReserve * (Math.pow(limitPrice / SPOT_PRICE, BASE_WEIGHT) - 1);

  const fitsFully = tradeSize <= maxFill;
  const filled = Math.min(tradeSize, maxFill);
  // Adaptive x-axis: keep the fill boundary around two-thirds of the chart so
  // the shaded beyond-limit region stays a corner, not the whole plot.
  const xMax = Math.max(tradeSize * 1.25, maxFill * 1.5, 1_000);

  // The plugin is registered once at chart creation, so it reads live values
  // through a ref instead of closing over state that would go stale.
  const drawState = useRef({ maxFill, xMax, limitPrice, allowPartial, alphaReserve, taoReserve });
  drawState.current = { maxFill, xMax, limitPrice, allowPartial, alphaReserve, taoReserve };

  // Region tint, fill-boundary guide, and direct curve labels in the style of
  // the v431 release graphs: uppercase FiraCode annotations drawn in-plot.
  const annotationPlugin = useMemo<Plugin<'line'>>(
    () => ({
      id: 'limitAnnotations',
      beforeDatasetsDraw(chart) {
        const { maxFill, limitPrice, allowPartial } = drawState.current;
        const { ctx, chartArea, scales } = chart;
        const xScale = scales.x;
        const yScale = scales.y;
        if (!xScale || !yScale) return;

        const xBoundary = xScale.getPixelForValue(maxFill);

        ctx.save();
        ctx.font = GRAPH_FONT;

        // Beyond-limit region
        ctx.fillStyle = ACCENT_REGION;
        ctx.fillRect(xBoundary, chartArea.top, chartArea.right - xBoundary, chartArea.height);
        ctx.fillStyle = ACCENT;
        ctx.textAlign = 'center';
        const cx = (xBoundary + chartArea.right) / 2;
        ctx.fillText('BEYOND LIMIT', cx, chartArea.top + 14);
        ctx.fillText(allowPartial ? 'NOT FILLED' : 'ORDER FAILS', cx, chartArea.top + 28);

        // Vertical fill-boundary guide
        ctx.strokeStyle = INK_FAINT;
        ctx.lineWidth = 1;
        ctx.setLineDash([4, 4]);
        ctx.beginPath();
        ctx.moveTo(xBoundary, chartArea.top);
        ctx.lineTo(xBoundary, chartArea.bottom);
        ctx.stroke();
        ctx.setLineDash([]);
        ctx.fillStyle = INK;
        ctx.textAlign = 'left';
        ctx.fillText(`MAX FILL ${formatTao(maxFill).toUpperCase()}`, xBoundary + 6, chartArea.bottom - 8);

        // Limit price label
        const yLimit = yScale.getPixelForValue(limitPrice);
        ctx.textAlign = 'left';
        ctx.fillText('LIMIT PRICE', chartArea.left + 6, yLimit - 6);

        ctx.restore();
      },
      afterDatasetsDraw(chart) {
        const { xMax, alphaReserve, taoReserve } = drawState.current;
        const { ctx, scales } = chart;
        const xScale = scales.x;
        const yScale = scales.y;
        if (!xScale || !yScale) return;

        // Direct labels beside each curve, at ~40% of the x-range where the
        // marginal and average curves have separated.
        const xLabel = xMax * 0.4;
        const xPx = xScale.getPixelForValue(xLabel);
        const execY = yScale.getPixelForValue(executionPrice(alphaReserve, taoReserve, xLabel));

        ctx.save();
        ctx.font = GRAPH_FONT;
        ctx.textAlign = 'left';
        // The marginal curve rises left-to-right, so hang the label below its
        // left anchor point — the curve pulls up and away from the text.
        const marginalY = yScale.getPixelForValue(
          poolPriceAfter(alphaReserve, taoReserve, xLabel),
        );
        ctx.fillStyle = INK_FAINT;
        ctx.fillText('MARGINAL POOL PRICE', xPx + 4, marginalY + 14);
        ctx.fillStyle = INK;
        ctx.fillText('AVG EXECUTION PRICE', xPx + 4, execY + 16);
        ctx.restore();
      },
    }),
    [],
  );

  const { fillCurve, cutCurve, poolCurve } = useMemo(() => {
    const fill: { x: number; y: number }[] = [];
    const cut: { x: number; y: number }[] = [];
    const pool: { x: number; y: number }[] = [];
    for (let i = 1; i <= CURVE_SAMPLES; i++) {
      const size = (xMax * i) / CURVE_SAMPLES;
      const exec = { x: size, y: executionPrice(alphaReserve, taoReserve, size) };
      if (size <= maxFill) fill.push(exec);
      else cut.push(exec);
      pool.push({ x: size, y: poolPriceAfter(alphaReserve, taoReserve, size) });
    }
    // Stitch the two exec-price segments together at the fill boundary.
    if (maxFill < xMax) {
      const boundary = { x: maxFill, y: executionPrice(alphaReserve, taoReserve, maxFill) };
      fill.push(boundary);
      cut.unshift(boundary);
    }
    return { fillCurve: fill, cutCurve: cut, poolCurve: pool };
  }, [alphaReserve, taoReserve, maxFill, xMax]);

  const data = useMemo(
    () => ({
      datasets: [
        {
          label: 'Avg execution price (fills)',
          data: fillCurve,
          borderColor: INK,
          backgroundColor: 'rgba(41, 41, 41, 0.03)',
          fill: 'origin' as const,
          tension: 0.2,
          pointRadius: 0,
          borderWidth: 1.75,
        },
        {
          label: 'Avg execution price (beyond limit)',
          data: cutCurve,
          borderColor: 'rgba(41, 41, 41, 0.3)',
          borderDash: [4, 4],
          tension: 0.2,
          pointRadius: 0,
          borderWidth: 1,
        },
        {
          label: 'Marginal pool price',
          data: poolCurve,
          borderColor: INK_FAINT,
          tension: 0.2,
          pointRadius: 0,
          borderWidth: 1,
        },
        {
          label: 'Limit price',
          data: [
            { x: 0, y: limitPrice },
            { x: xMax, y: limitPrice },
          ],
          borderColor: 'rgba(41, 41, 41, 0.5)',
          borderDash: [4, 4],
          pointRadius: 0,
          borderWidth: 1,
        },
        {
          label: 'Fill stops here',
          // Pool price after the filled amount: lands exactly on the limit
          // line when the order crosses it.
          data: [{ x: filled, y: poolPriceAfter(alphaReserve, taoReserve, filled) }],
          borderColor: ACCENT,
          backgroundColor: ACCENT,
          showLine: false,
          pointRadius: 4,
          pointStyle: 'circle' as const,
        },
      ],
    }),
    [fillCurve, cutCurve, poolCurve, limitPrice, filled, alphaReserve, taoReserve, xMax],
  );

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
              `trade ${(items[0]?.parsed.x ?? 0).toLocaleString('en-US', { maximumFractionDigits: 0 })} τ`,
            label: (ctx: { parsed: { y: number }; dataset: { label?: string } }) =>
              `${ctx.dataset.label ?? ''}: ${ctx.parsed.y.toFixed(6)} τ/α`,
          },
        },
      },
      scales: {
        x: {
          type: 'linear' as const,
          min: 0,
          max: xMax,
          grid: { color: GRID },
          border: { color: AXIS_BORDER },
          ticks: baseTicks({
            callback: (value: string | number) => Number(value).toLocaleString('en-US'),
          }),
          title: axisTitle('trade size (TAO in)'),
        },
        y: {
          grid: { color: GRID },
          border: { color: AXIS_BORDER },
          ticks: baseTicks({
            maxTicksLimit: 5,
            callback: (value: string | number) => Number(value).toFixed(4),
          }),
          title: axisTitle('price (τ per α)'),
        },
      },
    }),
    [xMax],
  );

  const outcome = fitsFully
    ? { value: 'fills fully', hint: `entire ${formatTao(tradeSize)} executes below the limit` }
    : allowPartial
      ? {
          value: 'fills partially',
          hint: `${formatTao(filled)} of ${formatTao(tradeSize)} executes, the rest is returned`,
        }
      : { value: 'fails', hint: 'allow_partial = false: the whole extrinsic errors out' };

  const executed = fitsFully ? tradeSize : allowPartial ? filled : 0;

  return (
    <ExplainerPanel
      title="Limit-price slippage protection"
      tag="add_stake_limit · balancer pool"
      caption={
        <>
          add_stake_limit stops filling once the pool price reaches your limit (
          <a href="/code/pallets/swap/src/pallet/balancer.rs#L18-L22" className="underline">
            pallets/swap/src/pallet/balancer.rs
          </a>
          ): the maximum TAO that fits is ∆y = y·((p′/p)^w1 − 1).
        </>
      }
    >
      {/* Chart */}
      <div className="h-72">
        <Line data={data} options={options} plugins={[annotationPlugin]} />
      </div>
      <p className="mt-2 max-w-2xl text-[0.6875rem] leading-relaxed text-mute">
        The thin curve is the marginal pool price, which meets the dashed limit line exactly at the
        fill boundary; the shaded execution-price curve is what you actually pay on average.
        remove_stake_limit is symmetric with ∆x = x·((p/p′)^w2 − 1).
      </p>

      {/* Result summary */}
      <div className="mt-8 border-t border-line pt-4">
        <p className="font-mono text-lg leading-snug" style={{ color: INK }}>
          {fitsFully ? (
            <>
              {formatTao(executed)} <span className="text-mute">executes in full</span>
            </>
          ) : allowPartial ? (
            <>
              <span style={{ color: ACCENT }}>{formatTao(executed)}</span>{' '}
              <span className="text-mute">of</span> {formatTao(tradeSize)}{' '}
              <span className="text-mute">executes</span>
            </>
          ) : (
            <>
              <span style={{ color: ACCENT }}>0 τ</span> <span className="text-mute">of</span>{' '}
              {formatTao(tradeSize)} <span className="text-mute">executes — order fails</span>
            </>
          )}
        </p>
        <div className="mt-4 grid grid-cols-2 gap-x-8 gap-y-4 sm:grid-cols-4">
          <ExplainerStat
            label="Limit price"
            value={formatPrice(limitPrice)}
            hint={`spot × (1 + ${tolerancePct.toFixed(1)}%)`}
          />
          <ExplainerStat label="Max fill at limit" value={formatTao(maxFill)} hint="∆y = y·((p′/p)^w1 − 1)" />
          <ExplainerStat
            label="Requested trade"
            value={formatTao(tradeSize)}
            hint={fitsFully ? 'within the fill region' : 'crosses the limit price'}
          />
          <ExplainerStat label="Outcome" value={outcome.value} hint={outcome.hint} accent={!fitsFully} />
        </div>
      </div>

      {/* Controls */}
      <div className="mt-8 border-t border-line pt-4 pb-1">
        <ExplainerToggle
          label="allow_partial"
          options={[
            { id: 'partial', label: 'true (partial fill)' },
            { id: 'kill', label: 'false (fill or kill)', accent: true },
          ]}
          value={allowPartial ? 'partial' : 'kill'}
          onChange={(id) => setAllowPartial(id === 'partial')}
        />
        <div className="mt-6 grid gap-x-8 gap-y-5 sm:grid-cols-3">
          <ExplainerSlider
            label="tolerance above spot"
            value={tolerancePct}
            min={0.1}
            max={5}
            step={0.1}
            display={`${tolerancePct.toFixed(1)}%`}
            onChange={setTolerancePct}
          />
          <ExplainerSlider
            label="trade size (∆y TAO in)"
            value={tradeSize}
            min={100}
            max={MAX_TRADE}
            step={100}
            display={formatTao(tradeSize)}
            onChange={setTradeSize}
          />
          <ExplainerSlider
            label="pool TAO reserve (y)"
            value={poolDepth}
            min={1_000}
            max={500_000}
            step={1_000}
            display={formatTao(poolDepth)}
            onChange={setPoolDepth}
          />
        </div>
      </div>
    </ExplainerPanel>
  );
}
