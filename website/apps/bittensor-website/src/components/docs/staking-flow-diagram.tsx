'use client';

import { useState } from 'react';
import { ExplainerPanel, ExplainerStat, ExplainerToggle } from './explainer-panel';
import { INK } from './chart-theme';

// Illustrative pool: 10,000 τ / 200,000 α → spot price 0.05 τ/α.
const POOL_TAO = 10_000;
const POOL_ALPHA = 200_000;
const COLDKEY_BALANCE = 1_000;
const STAKE_TAO = 100;
const UNSTAKE_ALPHA = 2_000;
// Default FeeRate ≈ 328/65535 ≈ 0.05% of the input side.
const FEE_RATE = 0.0005;

// Constant-product approximation of the balancer 0.5/0.5 pool:
// out = reserve_out × Δin / (reserve_in + Δin).
function poolOut(reserveOut: number, reserveIn: number, deltaIn: number): number {
  return (reserveOut * deltaIn) / (reserveIn + deltaIn);
}

const SPOT_PRICE = POOL_TAO / POOL_ALPHA;

const STAKE_FEE = STAKE_TAO * FEE_RATE;
const STAKE_NET = STAKE_TAO - STAKE_FEE;
const STAKE_ALPHA_OUT = poolOut(POOL_ALPHA, POOL_TAO, STAKE_NET);
const STAKE_PRICE_AFTER = (POOL_TAO + STAKE_NET) / (POOL_ALPHA - STAKE_ALPHA_OUT);
const STAKE_EFF_PRICE = STAKE_NET / STAKE_ALPHA_OUT;

const UNSTAKE_FEE = UNSTAKE_ALPHA * FEE_RATE;
const UNSTAKE_NET = UNSTAKE_ALPHA - UNSTAKE_FEE;
const UNSTAKE_TAO_OUT = poolOut(POOL_TAO, POOL_ALPHA, UNSTAKE_NET);
const UNSTAKE_PRICE_AFTER = (POOL_TAO - UNSTAKE_TAO_OUT) / (POOL_ALPHA + UNSTAKE_NET);
const UNSTAKE_EFF_PRICE = UNSTAKE_TAO_OUT / UNSTAKE_NET;

function fmt(value: number, digits = 2): string {
  return value.toLocaleString('en-US', {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  });
}

type Mode = 'stake' | 'unstake';
type NodeId = 'coldkey' | 'fee' | 'pool' | 'position';

interface StepInfo {
  active: NodeId;
  text: string;
}

const STAKE_STEPS: StepInfo[] = [
  {
    active: 'coldkey',
    text: `Coldkey signs add_stake(hotkey, netuid, ${STAKE_TAO} τ) — ${STAKE_TAO} τ leaves the coldkey's free balance into the subnet account.`,
  },
  {
    active: 'fee',
    text: `Swap fee ${STAKE_TAO} τ × FeeRate/65535 ≈ ${fmt(STAKE_FEE)} τ is carved off the TAO input and paid to the block author.`,
  },
  {
    active: 'pool',
    text: `The remaining ${fmt(STAKE_NET)} τ enters the TAO reserve and ${fmt(STAKE_ALPHA_OUT, 1)} α exits the alpha reserve — the price moves ${fmt(SPOT_PRICE, 4)} → ${fmt(STAKE_PRICE_AFTER, 4)} against you as the order fills.`,
  },
  {
    active: 'position',
    text: `The ${fmt(STAKE_ALPHA_OUT, 1)} α is credited to the (hotkey, coldkey, netuid) stake position; the hotkey's stake weight rises.`,
  },
  {
    active: 'position',
    text: `From now on the position's value floats with the pool price and earns the validator's emissions, minus the validator's take.`,
  },
];

const UNSTAKE_STEPS: StepInfo[] = [
  {
    active: 'position',
    text: `Coldkey signs remove_stake(hotkey, netuid, ${fmt(UNSTAKE_ALPHA, 0)} α) — the alpha is debited from the (hotkey, coldkey, netuid) stake position.`,
  },
  {
    active: 'fee',
    text: `Swap fee ${fmt(UNSTAKE_ALPHA, 0)} α × FeeRate/65535 ≈ ${fmt(UNSTAKE_FEE, 0)} α is taken from the alpha input, then swapped fee-free into TAO for the block author.`,
  },
  {
    active: 'pool',
    text: `The remaining ${fmt(UNSTAKE_NET, 0)} α enters the alpha reserve and ${fmt(UNSTAKE_TAO_OUT)} τ exits the TAO reserve — the price moves ${fmt(SPOT_PRICE, 4)} → ${fmt(UNSTAKE_PRICE_AFTER, 4)} against you.`,
  },
  {
    active: 'coldkey',
    text: `${fmt(UNSTAKE_TAO_OUT)} τ is transferred from the subnet account to the coldkey's free balance.`,
  },
  {
    active: 'position',
    text: `Any alpha left in the position keeps floating with the pool price and earning emissions.`,
  },
];

interface NodeContent {
  id: NodeId;
  label: string;
  lines: { name: string; value: string }[];
}

function stakeNodes(reached: (s: number) => boolean): NodeContent[] {
  return [
    {
      id: 'coldkey',
      label: 'coldkey free balance',
      lines: [
        {
          name: 'balance',
          value: reached(0)
            ? `${fmt(COLDKEY_BALANCE - STAKE_TAO)} τ`
            : `${fmt(COLDKEY_BALANCE)} τ`,
        },
        { name: 'signs', value: `add_stake(${STAKE_TAO} τ)` },
      ],
    },
    {
      id: 'fee',
      label: 'swap fee → block author',
      lines: [
        { name: 'fee (0.05%)', value: reached(1) ? `${fmt(STAKE_FEE)} τ` : '—' },
        { name: 'into pool', value: reached(1) ? `${fmt(STAKE_NET)} τ` : '—' },
      ],
    },
    {
      id: 'pool',
      label: 'balancer pool',
      lines: [
        {
          name: 'τ reserve',
          value: reached(2) ? `${fmt(POOL_TAO + STAKE_NET)} τ` : `${fmt(POOL_TAO)} τ`,
        },
        {
          name: 'α reserve',
          value: reached(2)
            ? `${fmt(POOL_ALPHA - STAKE_ALPHA_OUT, 1)} α`
            : `${fmt(POOL_ALPHA, 0)} α`,
        },
        {
          name: 'price',
          value: reached(2) ? `${fmt(STAKE_PRICE_AFTER, 4)} τ/α` : `${fmt(SPOT_PRICE, 4)} τ/α`,
        },
      ],
    },
    {
      id: 'position',
      label: '(hotkey, coldkey, netuid) stake',
      lines: [
        { name: 'alpha', value: reached(3) ? `${fmt(STAKE_ALPHA_OUT, 1)} α` : '0 α' },
        {
          name: 'value',
          value: reached(4) ? 'floats with pool price' : reached(3) ? 'credited' : '—',
        },
      ],
    },
  ];
}

function unstakeNodes(reached: (s: number) => boolean): NodeContent[] {
  return [
    {
      id: 'position',
      label: '(hotkey, coldkey, netuid) stake',
      lines: [
        {
          name: 'alpha',
          value: reached(0) ? '0 α' : `${fmt(UNSTAKE_ALPHA, 0)} α`,
        },
        { name: 'signs', value: `remove_stake(${fmt(UNSTAKE_ALPHA, 0)} α)` },
      ],
    },
    {
      id: 'fee',
      label: 'swap fee → block author',
      lines: [
        { name: 'fee (0.05%)', value: reached(1) ? `${fmt(UNSTAKE_FEE, 0)} α` : '—' },
        { name: 'into pool', value: reached(1) ? `${fmt(UNSTAKE_NET, 0)} α` : '—' },
      ],
    },
    {
      id: 'pool',
      label: 'balancer pool',
      lines: [
        {
          name: 'α reserve',
          value: reached(2)
            ? `${fmt(POOL_ALPHA + UNSTAKE_NET, 0)} α`
            : `${fmt(POOL_ALPHA, 0)} α`,
        },
        {
          name: 'τ reserve',
          value: reached(2) ? `${fmt(POOL_TAO - UNSTAKE_TAO_OUT)} τ` : `${fmt(POOL_TAO)} τ`,
        },
        {
          name: 'price',
          value: reached(2) ? `${fmt(UNSTAKE_PRICE_AFTER, 4)} τ/α` : `${fmt(SPOT_PRICE, 4)} τ/α`,
        },
      ],
    },
    {
      id: 'coldkey',
      label: 'coldkey free balance',
      lines: [
        {
          name: 'balance',
          value: reached(3)
            ? `${fmt(COLDKEY_BALANCE + UNSTAKE_TAO_OUT)} τ`
            : `${fmt(COLDKEY_BALANCE)} τ`,
        },
        { name: 'received', value: reached(3) ? `+${fmt(UNSTAKE_TAO_OUT)} τ` : '—' },
      ],
    },
  ];
}

export function StakingFlowDiagram() {
  const [mode, setMode] = useState<Mode>('stake');
  const [step, setStep] = useState(0);

  const steps = mode === 'stake' ? STAKE_STEPS : UNSTAKE_STEPS;
  const current = steps[step] ?? steps[0];
  const reached = (s: number) => step >= s;
  const nodes = mode === 'stake' ? stakeNodes(reached) : unstakeNodes(reached);

  const switchMode = (next: Mode) => {
    setMode(next);
    setStep(0);
  };

  const stepperButtonClass =
    'px-2.5 py-1 font-mono text-[0.6875rem] uppercase tracking-[0.08em] text-mute transition-colors hover:bg-panel disabled:opacity-30 disabled:hover:bg-bg';

  return (
    <ExplainerPanel
      title="What add_stake / remove_stake actually does"
      tag="add_stake.rs · swap_step.rs"
      caption="Step through the on-chain path from add_stake.rs → stake_into_subnet (stake_utils.rs) → the balancer swap in swap_step.rs. Illustrative pool: 10,000 τ / 200,000 α at 0.05 τ/α; the fee is 0.05% of the input side and 100% of it goes to the block author."
    >
      <div className="mb-6">
        <ExplainerToggle
          label="operation"
          options={[
            { id: 'stake', label: `stake (${STAKE_TAO} τ in)` },
            { id: 'unstake', label: `unstake (${fmt(UNSTAKE_ALPHA, 0)} α in)` },
          ]}
          value={mode}
          onChange={switchMode}
        />
      </div>

      <div className="flex flex-col items-stretch gap-3 md:flex-row md:gap-0">
        {nodes.map((node, i) => (
          <div key={node.id} className="contents">
            {i > 0 && (
              <div className="flex items-center justify-center font-mono text-sm text-mute md:px-2">
                <span className="hidden md:inline">→</span>
                <span className="md:hidden">↓</span>
              </div>
            )}
            <div
              className="flex-1 border-t-2 border-line pt-2 transition-colors duration-300"
              style={current.active === node.id ? { borderColor: INK } : undefined}
            >
              <p
                className={
                  'font-mono text-[0.625rem] uppercase tracking-[0.08em] transition-colors duration-300 ' +
                  (current.active === node.id ? '' : 'text-mute')
                }
                style={current.active === node.id ? { color: INK } : undefined}
              >
                {node.label}
              </p>
              <dl className="mt-2 space-y-1">
                {node.lines.map((line) => (
                  <div key={line.name} className="flex items-baseline justify-between gap-2">
                    <dt className="text-[0.6875rem] text-mute">{line.name}</dt>
                    <dd className="font-mono text-xs">{line.value}</dd>
                  </div>
                ))}
              </dl>
            </div>
          </div>
        ))}
      </div>

      <p className="mt-4 min-h-10 max-w-2xl text-[0.75rem] leading-relaxed text-mute">
        <span className="font-mono text-[0.625rem] uppercase tracking-[0.08em]">
          step {step + 1} / {steps.length} —{' '}
        </span>
        {current.text}
      </p>

      <div className="mt-3 inline-flex divide-x divide-line border border-line">
        <button
          type="button"
          onClick={() => setStep((s) => Math.max(0, s - 1))}
          disabled={step === 0}
          className={stepperButtonClass}
        >
          ← prev
        </button>
        <button
          type="button"
          onClick={() => setStep((s) => Math.min(steps.length - 1, s + 1))}
          disabled={step === steps.length - 1}
          className={stepperButtonClass}
        >
          next →
        </button>
        <button type="button" onClick={() => setStep(0)} className={stepperButtonClass}>
          reset
        </button>
      </div>

      <div className="mt-8 grid gap-x-8 gap-y-4 border-t border-line pt-4 sm:grid-cols-3">
        {mode === 'stake' ? (
          <>
            <ExplainerStat
              label="alpha received"
              value={`${fmt(STAKE_ALPHA_OUT, 1)} α`}
              hint={`${fmt(STAKE_NET / SPOT_PRICE, 0)} α at spot — slippage eats the rest`}
            />
            <ExplainerStat
              label="effective price"
              value={`${fmt(STAKE_EFF_PRICE, 5)} τ/α`}
              hint={`spot was ${fmt(SPOT_PRICE, 4)} τ/α`}
            />
            <ExplainerStat
              label="fee to block author"
              value={`${fmt(STAKE_FEE)} τ`}
              hint="input × FeeRate/65535, default ≈ 0.05%"
            />
          </>
        ) : (
          <>
            <ExplainerStat
              label="TAO received"
              value={`${fmt(UNSTAKE_TAO_OUT)} τ`}
              hint={`${fmt(UNSTAKE_NET * SPOT_PRICE, 2)} τ at spot — slippage eats the rest`}
            />
            <ExplainerStat
              label="effective price"
              value={`${fmt(UNSTAKE_EFF_PRICE, 5)} τ/α`}
              hint={`spot was ${fmt(SPOT_PRICE, 4)} τ/α`}
            />
            <ExplainerStat
              label="fee to block author"
              value={`${fmt(UNSTAKE_FEE, 0)} α`}
              hint="taken in alpha, swapped fee-free to τ"
            />
          </>
        )}
      </div>

      <p className="mt-4 border-t border-line pt-3 text-[0.75rem] text-mute">
        Root staking (netuid 0) skips all of this: there is no pool, so TAO is credited 1:1 as
        root stake — no swap fee, no slippage, no price movement.
      </p>
    </ExplainerPanel>
  );
}
