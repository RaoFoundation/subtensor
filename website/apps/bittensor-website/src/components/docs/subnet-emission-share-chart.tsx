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
import { useEmissionSnapshot } from '@/hooks/use-emission-snapshot';
import { formatPct, formatTao, subnetEmissionShares } from '@/lib/emission-math';
import { ACCENT, AXIS_BORDER, GRAPH_FONT, GRID, INK_FAINT, axisTitle, baseTicks } from './chart-theme';

ChartJS.register(CategoryScale, LinearScale, BarElement, Tooltip, Legend);

function subnetLabel(netuid: number, name: string): string {
  return `SN${netuid} ${name}`;
}

export function SubnetEmissionShareChart() {
  const {snapshot, loading} = useEmissionSnapshot();
  const [selectedIdx, setSelectedIdx] = useState(2);
  const [whatIfEma, setWhatIfEma] = useState<number | null>(null);
  const [whatIfBurn, setWhatIfBurn] = useState<number | null>(null);

  const rows = snapshot.topSubnets.slice(0, 8);
  const selected = rows[selectedIdx] ?? rows[0];

  const prices = useMemo(
    () => rows.map((r, i) => (i === selectedIdx && whatIfEma !== null ? whatIfEma : r.emaPrice)),
    [rows, selectedIdx, whatIfEma],
  );
  const burns = useMemo(
    () => rows.map((r, i) => (i === selectedIdx && whatIfBurn !== null ? whatIfBurn : r.minerBurned)),
    [rows, selectedIdx, whatIfBurn],
  );

  const shares = useMemo(() => subnetEmissionShares(prices, burns), [prices, burns]);
  const blockEmission = snapshot.blockEmissionTao;

  // The plugin is registered once at chart creation, so it reads live values
  // through a ref instead of closing over state that would go stale.
  const drawState = useRef({ shares, selectedIdx });
  drawState.current = { shares, selectedIdx };

  // Direct value labels at each bar end instead of a legend; the selected
  // (highlighted) bar carries the accent.
  const valueLabelPlugin = useMemo<Plugin<'bar'>>(
    () => ({
      id: 'barValueLabels',
      afterDatasetsDraw(chart) {
        const { shares, selectedIdx } = drawState.current;
        const meta = chart.getDatasetMeta(0);
        const { ctx } = chart;

        ctx.save();
        ctx.font = GRAPH_FONT;
        ctx.textAlign = 'left';
        ctx.textBaseline = 'middle';
        meta.data.forEach((bar, i) => {
          const share = shares[i];
          if (share === undefined) return;
          const selected = i === selectedIdx;
          ctx.fillStyle = selected ? ACCENT : INK_FAINT;
          const text = `${(share * 100).toFixed(1)}%${selected ? ' · SELECTED' : ''}`;
          ctx.fillText(text, bar.x + 6, bar.y);
        });
        ctx.restore();
      },
    }),
    [],
  );

  const data = useMemo(
    () => ({
      labels: rows.map((r) => subnetLabel(r.netuid, r.name).toUpperCase()),
      datasets: [
        {
          label: 'TAO share',
          data: shares.map((s) => s * 100),
          backgroundColor: rows.map((_, i) => (i === selectedIdx ? ACCENT : 'rgba(41,41,41,0.3)')),
          borderWidth: 0,
        },
      ],
    }),
    [rows, shares, selectedIdx],
  );

  const options = useMemo(
    () => ({
      responsive: true,
      maintainAspectRatio: false,
      indexAxis: 'y' as const,
      plugins: {
        legend: {display: false},
        tooltip: {
          callbacks: {
            label: (ctx: {parsed: {x: number}; dataIndex: number}) => {
              const row = rows[ctx.dataIndex];
              const tao = blockEmission * (ctx.parsed.x / 100);
              return [
                `${ctx.parsed.x.toFixed(1)}% of ${formatTao(blockEmission)}/block`,
                `${formatTao(tao, 4)}/block`,
                `EMA ${row.emaPrice.toFixed(4)} · burn ${formatPct(row.minerBurned, 0)}`,
              ];
            },
          },
        },
      },
      scales: {
        x: {
          // Headroom for the in-plot value labels beside the longest bar.
          max: Math.max(...shares.map((s) => s * 100)) * 1.35,
          grid: {color: GRID},
          border: {color: AXIS_BORDER},
          ticks: baseTicks({
            callback: (v: number | string) => `${Number(v).toFixed(0)}%`,
          }),
          title: axisTitle('share of TAO emission'),
        },
        y: {
          grid: {display: false},
          border: {color: AXIS_BORDER},
          ticks: baseTicks({autoSkip: false, maxTicksLimit: 12}),
        },
      },
      onClick: (_: unknown, elements: {index: number}[]) => {
        if (elements[0]) {
          setSelectedIdx(elements[0].index);
          setWhatIfEma(null);
          setWhatIfBurn(null);
        }
      },
    }),
    [rows, shares, blockEmission],
  );

  const displayEma = whatIfEma ?? selected?.emaPrice ?? 0;
  const displayBurn = whatIfBurn ?? selected?.minerBurned ?? 0;

  return (
    <ExplainerPanel
      title="Subnet TAO shares (finney, price-EMA)"
      caption="Live top subnets by spot price. Bar width = share_i = p_i×(1−b_i) / Σ. Click a bar to inspect; sliders run what-if on the selected subnet."
    >
      <div className="h-56">
        {loading ? (
          <div className="flex h-full items-center justify-center text-sm text-mute">Loading snapshot…</div>
        ) : (
          <Bar data={data} options={options} plugins={[valueLabelPlugin]} />
        )}
      </div>

      {selected && (
        <>
          <div className="mt-6 grid grid-cols-2 gap-x-8 gap-y-4 border-t border-line pt-4 sm:grid-cols-4">
            <ExplainerStat
              label={subnetLabel(selected.netuid, selected.name)}
              value={formatPct(shares[selectedIdx] ?? 0)}
              hint={`${formatTao(blockEmission * (shares[selectedIdx] ?? 0), 4)}/block`}
              accent
            />
            <ExplainerStat label="EMA price (p)" value={selected.emaPrice.toFixed(4)} hint={`Spot ${selected.spotPrice.toFixed(4)} τ/α`} />
            <ExplainerStat label="Miner burn (b)" value={formatPct(selected.minerBurned, 1)} hint="Last tempo withheld share" />
            <ExplainerStat
              label="Without burn penalty"
              value={formatPct(selected.emaPrice / (snapshot.emaPriceSum || 1), 1)}
              hint="Unweighted EMA share (approx)"
            />
          </div>

          <div className="mt-6 grid gap-x-8 gap-y-5 border-t border-line pt-4 sm:grid-cols-2">
            <ExplainerSlider
              label={`What-if EMA for SN${selected.netuid}`}
              value={displayEma}
              min={0.01}
              max={1}
              step={0.005}
              display={displayEma.toFixed(4)}
              onChange={(v) => setWhatIfEma(v)}
            />
            <ExplainerSlider
              label={`What-if miner burn for SN${selected.netuid}`}
              value={displayBurn}
              min={0}
              max={1}
              step={0.05}
              display={formatPct(displayBurn, 0)}
              onChange={(v) => setWhatIfBurn(v)}
            />
          </div>

          {selected.minerBurned > 0.1 && (
            <p className="mt-3 text-[0.8125rem] text-mute">
              SN{selected.netuid} carries a {formatPct(selected.minerBurned, 0)} burn penalty — roughly half
              its unpenalized EMA share is redistributed to subnets like Chutes and Affine with b=0.
            </p>
          )}
        </>
      )}
    </ExplainerPanel>
  );
}
