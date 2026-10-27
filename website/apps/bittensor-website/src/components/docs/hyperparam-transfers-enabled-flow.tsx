'use client';

import { useState } from 'react';
import { ExplainerPanel, ExplainerStat, ExplainerToggle } from './explainer-panel';
import { ACCENT, INK } from './chart-theme';

const TRANSFER_AMOUNT = 25;
const INITIAL_A = 100;
const INITIAL_B = 40;

function formatAlpha(value: number): string {
  return `${value.toFixed(0)} α`;
}

type LastAction =
  | { kind: 'idle' }
  | { kind: 'transferred' }
  | { kind: 'blocked' }
  | { kind: 'moved' };

function actionMessage(action: LastAction): string {
  switch (action.kind) {
    case 'idle':
      return 'Try a transfer with the toggle on and off.';
    case 'transferred':
      return `transfer_stake succeeded — ${TRANSFER_AMOUNT} α changed coldkeys. Any conviction lock follows the stake proportionally.`;
    case 'blocked':
      return 'transfer_stake failed with TransferDisallowed — TransferToggle is false on this subnet.';
    case 'moved':
      return 'move_stake succeeded — same coldkey, so the flag is never consulted.';
    default: {
      const exhaustive: never = action;
      return exhaustive;
    }
  }
}

export function HyperparamTransfersEnabledFlow() {
  const [enabled, setEnabled] = useState(true);
  const [stakeA, setStakeA] = useState(INITIAL_A);
  const [stakeB, setStakeB] = useState(INITIAL_B);
  const [action, setAction] = useState<LastAction>({ kind: 'idle' });

  // Mirrors the TransferToggle check in stake_utils.rs: transfer_stake fails
  // with TransferDisallowed when the toggle is off; same-coldkey moves skip it.
  const attemptTransfer = () => {
    if (!enabled) {
      setAction({ kind: 'blocked' });
      return;
    }
    if (stakeA < TRANSFER_AMOUNT) return;
    setStakeA((v) => v - TRANSFER_AMOUNT);
    setStakeB((v) => v + TRANSFER_AMOUNT);
    setAction({ kind: 'transferred' });
  };

  const attemptMove = () => {
    setAction({ kind: 'moved' });
  };

  const reset = () => {
    setStakeA(INITIAL_A);
    setStakeB(INITIAL_B);
    setAction({ kind: 'idle' });
  };

  const buttonClass = 'border border-line bg-bg px-3 py-1.5 font-mono text-xs hover:bg-panel';
  const maxStake = INITIAL_A + INITIAL_B;

  const coldkeyColumn = (label: string, stake: number) => (
    <div className="flex-1 border-t border-line pt-2">
      <p className="font-mono text-[0.625rem] uppercase tracking-[0.08em] text-mute">{label}</p>
      <p className="mt-1 font-mono text-sm">{formatAlpha(stake)}</p>
      <div className="mt-2 h-1.5 bg-[rgba(41,41,41,0.08)]">
        <div
          className="h-full transition-all duration-500"
          style={{ width: `${(stake / maxStake) * 100}%`, backgroundColor: INK }}
        />
      </div>
    </div>
  );

  return (
    <ExplainerPanel
      title="Stake flow under transfers_enabled"
      caption={
        <>
          <a
            href="/code/pallets/subtensor/src/macros/dispatches.rs#L1315-L1331"
            className="underline"
          >
            transfer_stake
          </a>{' '}
          moves alpha to a different coldkey and is the only path that checks TransferToggle:
          with the toggle off it fails with TransferDisallowed. Moving stake between hotkeys
          under the same coldkey (move_stake, swap_stake) never consults the flag, and neither
          does staking or unstaking — the stake is pinned to its coldkey, not trapped.
        </>
      }
    >
      <div className="flex items-stretch gap-6">
        {coldkeyColumn('coldkey A', stakeA)}
        <div className="flex flex-col items-center justify-center px-1">
          <span className="font-mono text-[0.625rem] uppercase tracking-[0.08em] text-mute">
            transfer_stake
          </span>
          <span
            className={
              'font-mono text-lg leading-none transition-opacity duration-300 ' +
              (enabled ? '' : 'line-through')
            }
            style={enabled ? undefined : { color: ACCENT }}
          >
            →
          </span>
          <span
            className="font-mono text-[0.625rem] uppercase tracking-[0.08em]"
            style={enabled ? undefined : { color: ACCENT }}
          >
            {enabled ? <span className="text-mute">allowed</span> : 'TransferDisallowed'}
          </span>
        </div>
        {coldkeyColumn('coldkey B', stakeB)}
      </div>

      <p
        className="mt-4 font-mono text-xs"
        style={action.kind === 'blocked' ? { color: ACCENT } : undefined}
      >
        <span className={action.kind === 'blocked' ? '' : 'text-mute'}>
          {actionMessage(action)}
        </span>
      </p>

      <div className="mt-8 border-t border-line pt-4">
        <div className="flex flex-wrap items-center gap-x-6 gap-y-3">
          <ExplainerToggle
            label="transfers_enabled"
            options={[
              { id: 'on', label: 'true' },
              { id: 'off', label: 'false', accent: true },
            ]}
            value={enabled ? 'on' : 'off'}
            onChange={(id) => setEnabled(id === 'on')}
          />
          <div className="flex flex-wrap gap-2">
            <button type="button" onClick={attemptTransfer} className={buttonClass}>
              Transfer {TRANSFER_AMOUNT} α to coldkey B
            </button>
            <button type="button" onClick={attemptMove} className={buttonClass}>
              Move within coldkey A
            </button>
            <button type="button" onClick={reset} className={buttonClass + ' text-mute'}>
              Reset
            </button>
          </div>
        </div>
      </div>

      <div className="mt-8 border-t border-line pt-4">
        <div className="grid grid-cols-1 gap-x-8 gap-y-4 sm:grid-cols-2">
          <ExplainerStat
            label="transfer_stake (A → B)"
            value={enabled ? 'allowed' : 'blocked'}
            hint={enabled ? 'Both subnets must allow it for cross-subnet moves' : 'TransferDisallowed'}
            accent={!enabled}
          />
          <ExplainerStat
            label="move_stake / swap_stake / unstake"
            value="always allowed"
            hint="Same coldkey — the flag is never checked"
          />
        </div>
      </div>
    </ExplainerPanel>
  );
}
