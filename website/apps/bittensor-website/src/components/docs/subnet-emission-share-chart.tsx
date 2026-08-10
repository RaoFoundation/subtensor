'use client';

import {useMemo, useRef, useState} from 'react';
import {
  Chart as ChartJS,
  CategoryScale,
  LinearScale,
  BarElement,
  Tooltip,
  Legend,
  type Plugin,
} from 'chart.js';
import {Bar} from 'react-chartjs-2';
import {ExplainerPanel, ExplainerSlider, ExplainerStat} from './explainer-panel';
import {useEmissionSnapshot} from '@/hooks/use-emission-snapshot';
import {formatPct, formatTao, subnetEmissionShares} from '@/lib/emission-math';
import {
  ACCENT,
  AXIS_BORDER,
  GRAPH_FONT,
  GRID,
  INK_FAINT,
  axisTitle,
  baseTicks,
} from './chart-theme';

ChartJS.register(CategoryScale, LinearScale, BarElement, Tooltip, Legend);

function subnetLabel(netuid: number, name: string): string {
  return `SN${netuid} ${name}`;
}

export function SubnetEmissionShareChart() {
  const {snapshot, loading} = useEmissionSnapshot();
  const [selectedIdx, setSelectedIdx] = useState(2);
  const [whatIfEma, setWhatIfEma] = useState<number | null>(null);

  const rows = useMemo(() => snapshot.topSubnets.slice(0, 8), [snapshot.topSubnets]);
  const selected = rows[selectedIdx] ?? rows[0];

  const calculation = useMemo(() => {
    const inputs = snapshot.emissionInputs.map((input) => ({
      ...input,
      emaPrice:
        input.netuid === selected?.netuid && whatIfEma !== null ? whatIfEma : input.emaPrice,
    }));
    const result = subnetEmissionShares(
      inputs.map((input) => input.emaPrice),
      {
        emissionEnabled: inputs.map((input) => input.emissionEnabled),
        rank: snapshot.emissionGateRank,
        quantile: snapshot.emissionGateQuantile,
        exponent: snapshot.emissionGateExponent,
        gateBar: snapshot.emissionGateBar,
      },
    );
    const indexByNetuid = new Map(inputs.map((input, index) => [input.netuid, index]));
    const priceByNetuid = new Map(inputs.map((input) => [input.netuid, input.emaPrice]));
    return {indexByNetuid, priceByNetuid, ...result};
  }, [selected?.netuid, snapshot, whatIfEma]);
  const shares = useMemo(
    () =>
      rows.map((row) => {
        const index = calculation.indexByNetuid.get(row.netuid);
        return index === undefined ? 0 : calculation.shares[index];
      }),
    [calculation, rows],
  );
  const blockEmission = snapshot.blockEmissionTao;

  // The plugin is registered once at chart creation, so it reads live values
  // through a ref instead of closing over state that would go stale.
  const drawState = useRef({shares, selectedIdx});
  drawState.current = {shares, selectedIdx};

  // Direct value labels at each bar end instead of a legend; the selected
  // (highlighted) bar carries the accent.
  const valueLabelPlugin = useMemo<Plugin<'bar'>>(
    () => ({
      id: 'barValueLabels',
      afterDatasetsDraw(chart) {
        const {shares, selectedIdx} = drawState.current;
        const meta = chart.getDatasetMeta(0);
        const {ctx} = chart;

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
                `Price EMA ${calculation.priceByNetuid.get(row.netuid)?.toFixed(4) ?? '0.0000'}`,
              ];
            },
          },
        },
      },
      scales: {
        x: {
          // Headroom for the in-plot value labels beside the longest bar.
          max: Math.max(1, ...shares.map((s) => s * 100)) * 1.35,
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
        }
      },
    }),
    [rows, shares, blockEmission, calculation],
  );

  const displayEma = whatIfEma ?? selected?.emaPrice ?? 0;
  const selectedInputIdx = selected ? calculation.indexByNetuid.get(selected.netuid) : undefined;
  const selectedDemandShare =
    selectedInputIdx === undefined ? 0 : calculation.demandShares[selectedInputIdx];
  const selectedGateFactor =
    selectedInputIdx === undefined ? 0 : calculation.gateFactors[selectedInputIdx];

  return (
    <ExplainerPanel
      title='Subnet TAO share preview'
      caption="The slider keeps the snapshot's gate midpoint fixed, just as the chain does between 360-block updates. Click a bar, then move its price EMA to see how demand and the gate affect its final share."
    >
      <div className='h-56'>
        {loading ? (
          <div className='flex h-full items-center justify-center text-sm text-mute'>
            Loading snapshot…
          </div>
        ) : (
          <Bar data={data} options={options} plugins={[valueLabelPlugin]} />
        )}
      </div>

      {selected && (
        <>
          <div className='mt-6 grid grid-cols-2 gap-x-8 gap-y-4 border-t border-line pt-4 sm:grid-cols-4'>
            <ExplainerStat
              label={subnetLabel(selected.netuid, selected.name)}
              value={formatPct(shares[selectedIdx] ?? 0)}
              hint={`${formatTao(blockEmission * (shares[selectedIdx] ?? 0), 4)}/block`}
              accent
            />
            <ExplainerStat
              label='Price EMA'
              value={displayEma.toFixed(4)}
              hint={`Spot ${selected.spotPrice.toFixed(4)} τ/α`}
            />
            <ExplainerStat
              label='Share before gate'
              value={formatPct(selectedDemandShare, 1)}
              hint="This subnet's fraction of total price EMA"
            />
            <ExplainerStat
              label='Demand that passes'
              value={formatPct(selectedGateFactor, 1)}
              hint={`50% at the modeled ${formatPct(calculation.gateBar, 2)} midpoint`}
            />
          </div>

          <div className='mt-6 border-t border-line pt-4'>
            <ExplainerSlider
              label={`What-if EMA for SN${selected.netuid}`}
              value={displayEma}
              min={0}
              max={Math.max(1, selected.emaPrice * 4)}
              step={0.005}
              display={displayEma.toFixed(4)}
              onChange={(v) => setWhatIfEma(v)}
            />
          </div>
        </>
      )}
    </ExplainerPanel>
  );
}
