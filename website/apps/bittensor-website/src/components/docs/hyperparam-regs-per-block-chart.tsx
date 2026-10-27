'use client';

import { useMemo, useRef, useState } from 'react';
import {
  Chart as ChartJS,
  CategoryScale,
  LinearScale,
  BarElement,
  Tooltip,
  type Plugin,
} from 'chart.js';
import { Bar } from 'react-chartjs-2';
import { ExplainerPanel, ExplainerSlider, ExplainerStat } from './explainer-panel';
import {
  ACCENT,
  ACCENT_WASH,
  AXIS_BORDER,
  GRAPH_FONT,
  GRID,
  INK,
  axisTitle,
  baseTicks,
} from './chart-theme';

ChartJS.register(CategoryScale, LinearScale, BarElement, Tooltip);

// A bursty demand pattern: quiet blocks, then registration rushes.
const ARRIVALS_BASE = [1, 3, 0, 6, 2, 9, 1, 0, 4, 12, 3, 1];

export function HyperparamRegsPerBlockChart() {
  const [cap, setCap] = useState(3);
  const [surge, setSurge] = useState(1);

  const blocks = useMemo(() => {
    return ARRIVALS_BASE.map((base) => {
      const arriving = Math.round(base * surge);
      // RegistrationsThisBlock starts at 0 every block (reset in on_initialize),
      // so each block independently admits up to the cap.
      const accepted = Math.min(arriving, cap);
      return { arriving, accepted, rejected: arriving - accepted };
    });
  }, [cap, surge]);

  const totalAccepted = blocks.reduce((sum, b) => sum + b.accepted, 0);
  const totalRejected = blocks.reduce((sum, b) => sum + b.rejected, 0);
  const cappedBlocks = blocks.filter((b) => b.rejected > 0).length;

  // The plugin is registered once at chart creation, so it reads live values
  // through a ref instead of closing over state that would go stale.
  const drawState = useRef({ cap, blocks });
  drawState.current = { cap, blocks };

  // Cap threshold line and direct labels, replacing any legend.
  const annotationPlugin = useMemo<Plugin<'bar'>>(
    () => ({
      id: 'regsPerBlockAnnotations',
      afterDatasetsDraw(chart) {
        const { cap, blocks } = drawState.current;
        const { ctx, chartArea, scales } = chart;
        const xScale = scales.x;
        const yScale = scales.y;
        if (!xScale || !yScale) return;

        ctx.save();
        ctx.font = GRAPH_FONT;

        // The cap is the per-block threshold every bar is clipped to.
        const yCap = yScale.getPixelForValue(cap);
        ctx.strokeStyle = ACCENT;
        ctx.lineWidth = 1;
        ctx.setLineDash([6, 4]);
        ctx.beginPath();
        ctx.moveTo(chartArea.left, yCap);
        ctx.lineTo(chartArea.right, yCap);
        ctx.stroke();
        ctx.setLineDash([]);
        ctx.fillStyle = ACCENT;
        ctx.textAlign = 'left';
        ctx.fillText('MAX_REGS_PER_BLOCK CAP', chartArea.left + 6, yCap - 6);

        // Label the tallest overflowing bar's rejected portion.
        let tallest = -1;
        let tallestArriving = 0;
        blocks.forEach((b, i) => {
          if (b.rejected > 0 && b.arriving > tallestArriving) {
            tallest = i;
            tallestArriving = b.arriving;
          }
        });
        if (tallest >= 0) {
          const xPx = xScale.getPixelForValue(tallest);
          const yTop = yScale.getPixelForValue(tallestArriving);
          ctx.textAlign = 'center';
          ctx.fillText('REJECTED', xPx, Math.max(yTop - 6, chartArea.top + 10));
        }

        ctx.restore();
      },
    }),
    [],
  );

  const data = useMemo(
    () => ({
      labels: blocks.map((_, i) => `${i + 1}`),
      datasets: [
        {
          label: 'accepted (counter fills up to cap)',
          data: blocks.map((b) => b.accepted),
          backgroundColor: 'rgba(41, 41, 41, 0.85)',
          borderColor: INK,
          borderWidth: 1,
          stack: 'regs',
        },
        {
          label: 'rejected: TooManyRegistrationsThisBlock',
          data: blocks.map((b) => b.rejected),
          backgroundColor: ACCENT_WASH,
          borderColor: ACCENT,
          borderWidth: 1,
          stack: 'regs',
        },
      ],
    }),
    [blocks],
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
            title: (items: { dataIndex: number }[]) => {
              const idx = items[0]?.dataIndex ?? 0;
              return `Block ${idx + 1} \u00b7 ${blocks[idx]?.arriving ?? 0} arriving`;
            },
            label: (ctx: { dataset: { label?: string }; parsed: { y: number } }) =>
              `${ctx.dataset.label ?? ''}: ${ctx.parsed.y}`,
          },
        },
      },
      scales: {
        x: {
          stacked: true,
          grid: { color: GRID },
          border: { color: AXIS_BORDER },
          ticks: baseTicks({ maxTicksLimit: 12 }),
          title: axisTitle('consecutive blocks (counter resets each block)'),
        },
        y: {
          stacked: true,
          grid: { color: GRID },
          border: { color: AXIS_BORDER },
          ticks: baseTicks({ precision: 0 }),
          title: axisTitle('registration attempts'),
        },
      },
    }),
    [blocks],
  );

  return (
    <ExplainerPanel
      title="Per-block registration cap"
      caption={
        <>
          Each successful registration bumps the RegistrationsThisBlock counter; on_initialize
          resets it to zero next block. Once the counter reaches max_regs_per_block, further
          attempts in that block fail with TooManyRegistrationsThisBlock (
          <a
            href="/code/pallets/subtensor/src/coinbase/root.rs#L77-L188"
            className="underline"
          >
            do_root_register in coinbase/root.rs
          </a>
          ; checked_allowed_register reports the subnet closed for the rest of the block). The
          dark portion of each bar is what got in; the pale portion overflowed the cap.
        </>
      }
    >
      <div className="h-52">
        <Bar data={data} options={options} plugins={[annotationPlugin]} />
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-3">
        <ExplainerStat
          label="Accepted across 12 blocks"
          value={`${totalAccepted}`}
          hint={`at most ${cap} per block, however hot demand runs`}
        />
        <ExplainerStat
          label="Rejected this window"
          value={`${totalRejected}`}
          hint="TooManyRegistrationsThisBlock; callers can retry next block"
        />
        <ExplainerStat
          label="Blocks that hit the cap"
          value={`${cappedBlocks} of 12`}
          hint="counter resets to 0 every block, so bursts spread out instead of flooding"
        />
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-2">
        <ExplainerSlider
          label="max_regs_per_block (cap)"
          value={cap}
          min={1}
          max={12}
          step={1}
          display={`${cap}${cap === 1 ? ' (mainnet default)' : ''}`}
          onChange={setCap}
        />
        <ExplainerSlider
          label="Demand intensity"
          value={surge}
          min={0.5}
          max={3}
          step={0.5}
          display={`\u00d7${surge}`}
          onChange={setSurge}
        />
      </div>
    </ExplainerPanel>
  );
}
