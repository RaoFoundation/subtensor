'use client';

import { useState } from 'react';
import { ExplainerPanel, ExplainerSlider, ExplainerStat, ExplainerToggle } from './explainer-panel';

const OWNER_CUT_FRACTION = 11_796 / 65_535; // SubnetOwnerCut default, ≈18%

function formatAlpha(value: number): string {
  return `${value.toFixed(1)} α`;
}

function BarSegment({
  fraction,
  name,
  value,
  background,
}: {
  fraction: number;
  name: string;
  value: string;
  background: string;
}) {
  return (
    <div
      className="flex items-center justify-center overflow-hidden whitespace-nowrap transition-all duration-500"
      style={{ width: `${fraction * 100}%`, background }}
      title={`${name} ${value}`}
    >
      {fraction > 0.08 && (
        <span className="px-1 font-mono text-[0.625rem] tracking-[0.08em] text-white mix-blend-difference">
          {/* uppercase only the name: text-transform would turn α into Α */}
          <span className="uppercase">{name}</span> {value}
        </span>
      )}
    </div>
  );
}

function focusCaption(focus: string | undefined): string {
  switch (focus) {
    case 'owner_cut_enabled':
      return ' Focused on owner_cut_enabled: flip the highlighted toggle off and the owner segment vanishes — the full alpha_out flows to miners and validators, and the forgone cut is never stashed or paid retroactively.';
    case 'owner_cut_auto_lock_enabled':
      return ' Focused on owner_cut_auto_lock_enabled: flip the highlighted toggle on and the owner segment turns hatched — the same cut is paid, then immediately conviction-locked on the owner coldkey instead of staying liquid.';
    default:
      return '';
  }
}

export function HyperparamOwnerCut({ focus }: { focus?: string }) {
  const [tempoAlpha, setTempoAlpha] = useState(360);
  const [ownerCutEnabled, setOwnerCutEnabled] = useState(true);
  const [autoLockEnabled, setAutoLockEnabled] = useState(false);

  const ownerCut = ownerCutEnabled ? tempoAlpha * OWNER_CUT_FRACTION : 0;
  const ownerLocked = autoLockEnabled ? ownerCut : 0;
  const ownerLiquid = ownerCut - ownerLocked;
  const remainder = tempoAlpha - ownerCut;
  const miners = remainder / 2;
  const validators = remainder / 2;

  const total = tempoAlpha > 0 ? tempoAlpha : 1;

  const focusClass = (name: string) =>
    focus === name ? 'border-l-2 border-[var(--bt-fg)] pl-3' : '';

  return (
    <ExplainerPanel
      title="One tempo's alpha emission split"
      caption={`alpha_out accrued over one tempo, divided at the epoch. Owner cut is 18% (SubnetOwnerCut) when owner_cut_enabled; the remainder splits 50/50 between miners and validators + stakers (root proportion omitted for clarity).${focusCaption(focus)}`}
    >
      <div className="flex h-10 w-full border border-line">
        {ownerLiquid > 0 && (
          <BarSegment
            fraction={ownerLiquid / total}
            name="owner"
            value={formatAlpha(ownerLiquid)}
            background="rgb(41, 41, 41)"
          />
        )}
        {ownerLocked > 0 && (
          <BarSegment
            fraction={ownerLocked / total}
            name="owner (locked)"
            value={formatAlpha(ownerLocked)}
            background="repeating-linear-gradient(45deg, rgb(41, 41, 41), rgb(41, 41, 41) 4px, rgb(90, 90, 90) 4px, rgb(90, 90, 90) 8px)"
          />
        )}
        <BarSegment
          fraction={miners / total}
          name="miners"
          value={formatAlpha(miners)}
          background="rgba(41, 41, 41, 0.45)"
        />
        <BarSegment
          fraction={validators / total}
          name="validators + stakers"
          value={formatAlpha(validators)}
          background="rgba(41, 41, 41, 0.12)"
        />
      </div>

      <div className="mt-8 border-t border-line pt-4">
        <div className="grid grid-cols-2 gap-x-8 gap-y-4 sm:grid-cols-3">
          <ExplainerStat
            label="Owner cut"
            value={formatAlpha(ownerCut)}
            hint={
              ownerCut === 0
                ? 'Disabled — redistributed to participants'
                : autoLockEnabled
                  ? 'Staked to owner, then conviction-locked'
                  : 'Staked to owner hotkey, liquid'
            }
          />
          <ExplainerStat label="Miners" value={formatAlpha(miners)} hint="50% of the remainder" />
          <ExplainerStat
            label="Validators + stakers"
            value={formatAlpha(validators)}
            hint="50% of the remainder"
          />
        </div>
      </div>

      <div className="mt-8 border-t border-line pt-4 pb-1">
        <div className="flex flex-wrap gap-x-10 gap-y-4">
          <div className={focusClass('owner_cut_enabled')}>
            <ExplainerToggle
              label="owner_cut_enabled"
              options={[
                { id: 'on', label: 'true' },
                { id: 'off', label: 'false' },
              ]}
              value={ownerCutEnabled ? 'on' : 'off'}
              onChange={(id) => setOwnerCutEnabled(id === 'on')}
            />
          </div>
          <div className={focusClass('owner_cut_auto_lock_enabled')}>
            <ExplainerToggle
              label="owner_cut_auto_lock_enabled"
              options={[
                { id: 'on', label: 'true' },
                { id: 'off', label: 'false' },
              ]}
              value={autoLockEnabled ? 'on' : 'off'}
              onChange={(id) => setAutoLockEnabled(id === 'on')}
            />
          </div>
        </div>
        <div className="mt-6 grid gap-x-8 gap-y-5 sm:grid-cols-2">
          <ExplainerSlider
            label="alpha_out this tempo"
            value={tempoAlpha}
            min={0}
            max={720}
            step={10}
            display={formatAlpha(tempoAlpha)}
            onChange={setTempoAlpha}
          />
        </div>
      </div>
    </ExplainerPanel>
  );
}
