'use client';

import { useState } from 'react';
import { ExplainerPanel, ExplainerStat } from './explainer-panel';
import { ACCENT, ACCENT_WASH, INK } from './chart-theme';

// Illustrative scenario: 500 τ stake into a 50,000 τ / 1,000,000 α pool
// (spot 0.05 τ/α); the attacker front-runs with 2,000 τ. Constant-product
// approximation of the balancer 0.5/0.5 pool: out = x·Δy/(y+Δy).
const POOL_TAO = 50_000;
const POOL_ALPHA = 1_000_000;
const VICTIM_TAO = 500;
const ATTACKER_TAO = 2_000;

function poolOut(reserveOut: number, reserveIn: number, deltaIn: number): number {
  return (reserveOut * deltaIn) / (reserveIn + deltaIn);
}

const SPOT_PRICE = POOL_TAO / POOL_ALPHA;

// Lane B (shielded): the victim trades against the untouched pool.
const CLEAN_ALPHA = poolOut(POOL_ALPHA, POOL_TAO, VICTIM_TAO);
const CLEAN_PRICE = VICTIM_TAO / CLEAN_ALPHA;

// Lane A (plain): attacker buys first, victim fills at the pushed price,
// attacker sells back after.
const ATK_ALPHA = poolOut(POOL_ALPHA, POOL_TAO, ATTACKER_TAO);
const POOL_ALPHA_1 = POOL_ALPHA - ATK_ALPHA;
const POOL_TAO_1 = POOL_TAO + ATTACKER_TAO;
const PUSHED_SPOT = POOL_TAO_1 / POOL_ALPHA_1;
const SANDWICH_ALPHA = poolOut(POOL_ALPHA_1, POOL_TAO_1, VICTIM_TAO);
const SANDWICH_PRICE = VICTIM_TAO / SANDWICH_ALPHA;
const POOL_ALPHA_2 = POOL_ALPHA_1 - SANDWICH_ALPHA;
const POOL_TAO_2 = POOL_TAO_1 + VICTIM_TAO;
const ATK_TAO_OUT = poolOut(POOL_TAO_2, POOL_ALPHA_2, ATK_ALPHA);
const ATK_PROFIT = ATK_TAO_OUT - ATTACKER_TAO;

const ALPHA_LOST = CLEAN_ALPHA - SANDWICH_ALPHA;
const PRICE_WORSE_PCT = ((SANDWICH_PRICE - CLEAN_PRICE) / CLEAN_PRICE) * 100;

function fmt(value: number, digits = 0): string {
  return value.toLocaleString('en-US', {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  });
}

interface Chip {
  id: string;
  text: string;
  appearsAt: number;
  focusAt: number;
  muted?: boolean;
  /** Attack path: sandwich legs and the victim's worsened fill get ACCENT. */
  accent?: boolean;
}

interface Lane {
  label: string;
  phases: [Chip[], Chip[], Chip[]];
}

const PHASE_LABELS = ['mempool', 'block N', 'block N+1'] as const;

const PLAIN_LANE: Lane = {
  label: 'lane A — plain submission',
  phases: [
    [
      { id: 'victim-tx', text: `add_stake(${VICTIM_TAO} τ) — plaintext`, appearsAt: 0, focusAt: 0 },
      {
        id: 'attacker-buy-tx',
        text: `attacker buy ${fmt(ATTACKER_TAO)} τ (higher tip)`,
        appearsAt: 1,
        focusAt: 1,
        accent: true,
      },
    ],
    [
      {
        id: 'attacker-buy',
        text: `attacker buy fills — price ${fmt(SPOT_PRICE, 4)} → ${fmt(PUSHED_SPOT, 4)}`,
        appearsAt: 2,
        focusAt: 2,
        accent: true,
      },
      {
        id: 'victim-fill',
        text: `victim fills @ ${fmt(SANDWICH_PRICE, 4)} τ/α — ${fmt(SANDWICH_ALPHA)} α`,
        appearsAt: 3,
        focusAt: 3,
        accent: true,
      },
    ],
    [
      {
        id: 'attacker-sell',
        text: `attacker sells ${fmt(ATK_ALPHA)} α — +${fmt(ATK_PROFIT)} τ profit`,
        appearsAt: 4,
        focusAt: 4,
        accent: true,
      },
    ],
  ],
};

const SHIELDED_LANE: Lane = {
  label: 'lane B — shielded submission',
  phases: [
    [
      {
        id: 'ciphertext',
        text: 'MevShield.submit_encrypted(0x8f3a…) — ciphertext, 8-block era',
        appearsAt: 0,
        focusAt: 1,
      },
    ],
    [
      { id: 'decrypt', text: 'author decrypts, includes inner extrinsic', appearsAt: 2, focusAt: 2 },
      {
        id: 'victim-fill-clean',
        text: `victim fills @ ${fmt(CLEAN_PRICE, 4)} τ/α — ${fmt(CLEAN_ALPHA)} α`,
        appearsAt: 3,
        focusAt: 3,
      },
    ],
    [{ id: 'nothing', text: 'nothing — no sandwich', appearsAt: 4, focusAt: 4, muted: true }],
  ],
};

const STEP_CAPTIONS: string[] = [
  `The victim submits a ${VICTIM_TAO} τ stake. Plain: the call sits readable in the public mempool. Shielded: the inner call is signed, encrypted to the chain's rotating ML-KEM-768 key with XChaCha20-Poly1305, and wrapped in MevShield.submit_encrypted — the mempool sees only ciphertext.`,
  `The attacker's bot scans the mempool. Plain: it reads the pending ${VICTIM_TAO} τ order and submits its own ${fmt(ATTACKER_TAO)} τ buy with a higher tip. Shielded: only an opaque blob — nothing to front-run. Both shielded signatures use a short 8-block era, so a stuck submission evicts quickly.`,
  `Block N is built. Plain: the attacker's buy lands first, pushing the pool price ${fmt(SPOT_PRICE, 4)} → ${fmt(PUSHED_SPOT, 4)}. Shielded: the block author decrypts the wrapper and includes the inner extrinsic in the block it builds.`,
  `The victim's trade executes. Plain: at ${fmt(SANDWICH_PRICE, 4)} τ/α — only ${fmt(SANDWICH_ALPHA)} α for ${VICTIM_TAO} τ. Shielded: at ${fmt(CLEAN_PRICE, 4)} τ/α — ${fmt(CLEAN_ALPHA)} α, the clean price.`,
  `Block N+1. Plain: the attacker sells ${fmt(ATK_ALPHA)} α back for ≈${fmt(ATK_TAO_OUT)} τ, pocketing ≈${fmt(ATK_PROFIT)} τ of the victim's slippage. Shielded: nothing follows — there was no sandwich.`,
];

function LaneRow({ lane, step }: { lane: Lane; step: number }) {
  return (
    <div>
      <p className="mb-2 font-mono text-[0.625rem] uppercase tracking-[0.08em] text-mute">
        {lane.label}
      </p>
      <div className="grid grid-cols-3 gap-x-4 gap-y-2">
        {lane.phases.map((chips, i) => (
          <div key={PHASE_LABELS[i]} className="min-h-16 border-t border-line pt-2">
            <div className="flex flex-col gap-2">
              {chips.map((chip) => {
                const focused = step === chip.focusAt;
                return (
                  <div
                    key={chip.id}
                    className={
                      'border-l-2 py-0.5 pl-2 font-mono text-[0.6875rem] leading-snug transition-all duration-300 ' +
                      (step >= chip.appearsAt ? 'opacity-100 ' : 'opacity-20 ') +
                      (chip.muted ? 'text-mute' : '')
                    }
                    style={{
                      borderColor: focused ? (chip.accent ? ACCENT : INK) : 'transparent',
                      color: chip.accent ? ACCENT : undefined,
                      backgroundColor: focused && chip.accent ? ACCENT_WASH : undefined,
                    }}
                  >
                    {chip.text}
                  </div>
                );
              })}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

export function MevShieldTimeline() {
  const [step, setStep] = useState(0);
  const lastStep = STEP_CAPTIONS.length - 1;
  const caption = STEP_CAPTIONS[step] ?? STEP_CAPTIONS[0];

  const stepperButtonClass =
    'px-2.5 py-1 font-mono text-[0.6875rem] uppercase tracking-[0.08em] text-mute transition-colors hover:bg-panel disabled:opacity-30 disabled:hover:bg-bg';

  return (
    <ExplainerPanel
      title="Plain vs MEV-shielded submission"
      tag="pallets/shield"
      caption={
        <>
          The same 500 τ stake into a 50,000 τ pool, submitted plain and shielded
          (docs/concepts/advanced.mdx,{' '}
          <a href="/code/pallets/shield/src/lib.rs" className="underline">
            pallets/shield
          </a>
          ). Plain, the order is readable in the mempool and gets sandwiched; shielded, the
          mempool holds only ciphertext that the block author decrypts at build time.
        </>
      }
    >
      <div className="mb-3 grid grid-cols-3 gap-x-4">
        {PHASE_LABELS.map((label) => (
          <p key={label} className="font-mono text-[0.625rem] uppercase tracking-[0.08em] text-mute">
            {label}
          </p>
        ))}
      </div>

      <div className="space-y-6">
        <LaneRow lane={PLAIN_LANE} step={step} />
        <LaneRow lane={SHIELDED_LANE} step={step} />
      </div>

      <p className="mt-4 min-h-14 max-w-2xl text-[0.75rem] leading-relaxed text-mute">
        <span className="font-mono text-[0.625rem] uppercase tracking-[0.08em]">
          step {step + 1} / {STEP_CAPTIONS.length} —{' '}
        </span>
        {caption}
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
          onClick={() => setStep((s) => Math.min(lastStep, s + 1))}
          disabled={step === lastStep}
          className={stepperButtonClass}
        >
          next →
        </button>
        <button type="button" onClick={() => setStep(0)} className={stepperButtonClass}>
          reset
        </button>
      </div>

      <div className="mt-8 grid gap-x-8 gap-y-4 border-t border-line pt-4 sm:grid-cols-3">
        <ExplainerStat
          label="plain — victim exec price"
          value={`${fmt(SANDWICH_PRICE, 4)} τ/α`}
          hint={`${fmt(SANDWICH_ALPHA)} α received`}
        />
        <ExplainerStat
          label="shielded — victim exec price"
          value={`${fmt(CLEAN_PRICE, 4)} τ/α`}
          hint={`${fmt(CLEAN_ALPHA)} α received`}
        />
        <ExplainerStat
          label="sandwich cost"
          value={`−${fmt(ALPHA_LOST)} α`}
          hint={`price ${fmt(PRICE_WORSE_PCT, 1)}% worse; attacker keeps ≈${fmt(ATK_PROFIT)} τ`}
        />
      </div>

      <p className="mt-4 border-t border-line pt-3 text-[0.75rem] text-mute">
        Shielding matters for large pool swaps with loose price limits — the cases where your own
        price impact is worth stealing. Root staking (netuid 0) has no pool, and small swaps move
        the price too little to sandwich; neither is worth shielding. SDK:{' '}
        <span className="font-mono">client.submit_shielded(intent, wallet)</span>.
      </p>
    </ExplainerPanel>
  );
}
