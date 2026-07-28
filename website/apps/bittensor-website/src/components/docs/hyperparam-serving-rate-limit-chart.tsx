'use client';

import { useMemo, useState } from 'react';
import { ExplainerPanel, ExplainerSlider, ExplainerStat } from './explainer-panel';
import { ACCENT, ACCENT_REGION, AXIS_BORDER, INK } from './chart-theme';

const TIMELINE_BLOCKS = 260;
const ATTEMPT_BLOCKS = [4, 20, 60, 72, 84, 130, 140, 190, 250];
const DEFAULT_LIMIT = 50;

type Attempt = { block: number; accepted: boolean; sinceLast: number | null };

// Mirrors axon_passes_rate_limit (pallets/subtensor/src/subnets/serving.rs):
// pass when rate_limit == 0 || last_serve == 0 || current − last >= rate_limit.
function simulate(rateLimit: number): Attempt[] {
  let lastServe = 0;
  return ATTEMPT_BLOCKS.map((block) => {
    const sinceLast = lastServe === 0 ? null : block - lastServe;
    const accepted = rateLimit === 0 || lastServe === 0 || block - lastServe >= rateLimit;
    if (accepted) lastServe = block;
    return { block, accepted, sinceLast };
  });
}

function wallClock(blocks: number): string {
  const seconds = blocks * 12;
  const minutes = Math.floor(seconds / 60);
  const rest = seconds % 60;
  if (minutes === 0) return `${seconds}s`;
  return rest === 0 ? `${minutes}m` : `${minutes}m ${rest}s`;
}

function attemptTitle(attempt: Attempt): string {
  const since = attempt.sinceLast === null ? 'first serve' : `${attempt.sinceLast} blocks since last accepted serve`;
  return `block ${attempt.block} — ${attempt.accepted ? 'accepted' : 'rejected (ServingRateLimitExceeded)'}, ${since}`;
}

export function HyperparamServingRateLimitChart() {
  const [rateLimit, setRateLimit] = useState(DEFAULT_LIMIT);
  const attempts = useMemo(() => simulate(rateLimit), [rateLimit]);
  const acceptedCount = attempts.filter((a) => a.accepted).length;
  const pct = (block: number) => (block / TIMELINE_BLOCKS) * 100;

  return (
    <ExplainerPanel
      title="serve_axon rate limit timeline"
      caption={
        <>
          One miner&apos;s serve_axon attempts on a block timeline. Solid ticks are accepted;
          short dashed ticks land inside the shaded cooldown that follows each accepted serve
          and fail with ServingRateLimitExceeded. The first-ever serve always passes, and a
          limit of 0 disables{' '}
          <a
            href="/code/pallets/subtensor/src/subnets/serving.rs#L165-L173"
            className="underline"
          >
            the check
          </a>{' '}
          entirely.
        </>
      }
    >
      <div className="relative h-20">
        {rateLimit > 0 &&
          attempts
            .filter((a) => a.accepted)
            .map((a) => (
              <div
                key={`cooldown-${a.block}`}
                className="absolute inset-y-0"
                style={{
                  left: `${pct(a.block)}%`,
                  width: `${pct(Math.min(rateLimit, TIMELINE_BLOCKS - a.block))}%`,
                  backgroundColor: ACCENT_REGION,
                }}
              />
            ))}
        {attempts.map((a) =>
          a.accepted ? (
            <div
              key={a.block}
              className="absolute inset-y-1 w-[3px] -translate-x-1/2"
              style={{ left: `${pct(a.block)}%`, backgroundColor: INK }}
              title={attemptTitle(a)}
            />
          ) : (
            <div
              key={a.block}
              className="absolute inset-y-4 w-0 -translate-x-1/2 border-l-2 border-dashed"
              style={{ left: `${pct(a.block)}%`, borderColor: ACCENT }}
              title={attemptTitle(a)}
            />
          ),
        )}
        {/* baseline */}
        <div className="absolute inset-x-0 bottom-0 border-b" style={{ borderColor: AXIS_BORDER }} />
      </div>
      <div className="relative mt-2 h-4 font-mono text-[0.625rem] text-mute">
        {[0, 60, 120, 180, 240].map((block) => (
          <span
            key={block}
            className={block === 0 ? 'absolute' : 'absolute -translate-x-1/2'}
            style={{ left: `${pct(block)}%` }}
          >
            {block}
          </span>
        ))}
        <span className="absolute right-0 uppercase tracking-[0.08em]">block</span>
      </div>
      <div className="mt-2 flex flex-wrap gap-x-4 gap-y-1 font-mono text-[0.625rem] uppercase tracking-[0.08em] text-mute">
        <span>&#9612; accepted serve</span>
        <span style={{ color: ACCENT }}>&#9482; rejected in cooldown</span>
      </div>

      <div className="mt-8 border-t border-line pt-4">
        <div className="grid grid-cols-2 gap-x-8 gap-y-4 sm:grid-cols-3">
          <ExplainerStat
            label="Cooldown"
            value={rateLimit === 0 ? 'disabled' : `${rateLimit} blocks`}
            hint={rateLimit === 0 ? 'Every attempt passes' : `≈ ${wallClock(rateLimit)} at 12s blocks`}
          />
          <ExplainerStat label="Accepted" value={`${acceptedCount} / ${attempts.length}`} />
          <ExplainerStat
            label="Rejected"
            value={String(attempts.length - acceptedCount)}
            hint="ServingRateLimitExceeded"
            accent={attempts.length - acceptedCount > 0}
          />
        </div>
      </div>

      <div className="mt-8 border-t border-line pt-4 pb-1">
        <div className="grid gap-x-8 gap-y-5 sm:grid-cols-2">
          <ExplainerSlider
            label="serving_rate_limit"
            value={rateLimit}
            min={0}
            max={100}
            step={5}
            display={rateLimit === 0 ? '0 (disabled)' : `${rateLimit} blocks ≈ ${wallClock(rateLimit)}`}
            onChange={setRateLimit}
          />
        </div>
      </div>
    </ExplainerPanel>
  );
}
