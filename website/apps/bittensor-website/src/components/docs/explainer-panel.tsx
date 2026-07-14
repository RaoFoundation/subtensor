'use client';

import { useId, type ReactNode } from 'react';
import { cn } from '@/lib/cn';
import { ACCENT, ACCENT_WASH, INK } from './chart-theme';

/**
 * Editorial figure frame: strong near-black top rule with an uppercase mono
 * title (and optional right-aligned micro-tag), a muted intro paragraph, the
 * content, and a light bottom rule. No enclosing box.
 */
export function ExplainerPanel({
  title,
  tag,
  caption,
  children,
  className,
}: {
  title: string;
  tag?: string;
  caption?: string;
  children: ReactNode;
  className?: string;
}) {
  return (
    <figure className={cn('not-prose my-10', className)}>
      <div className="border-t-2 pt-4" style={{ borderColor: INK }}>
        <div className="flex flex-wrap items-baseline justify-between gap-x-6 gap-y-1">
          <figcaption
            className="font-mono text-[0.8125rem] font-medium uppercase tracking-[0.1em]"
            style={{ color: INK }}
          >
            {title}
          </figcaption>
          {tag && (
            <span className="font-mono text-[0.625rem] uppercase tracking-[0.08em] text-mute">
              {tag}
            </span>
          )}
        </div>
        {caption && (
          <p className="mt-2 max-w-2xl text-[0.8125rem] leading-relaxed text-mute">{caption}</p>
        )}
      </div>
      <div className="mt-6">{children}</div>
      <div className="mt-6 border-b border-line" />
    </figure>
  );
}

export function ExplainerStat({
  label,
  value,
  hint,
  accent = false,
}: {
  label: string;
  value: string;
  hint?: string;
  accent?: boolean;
}) {
  return (
    <div>
      <p className="font-mono text-[0.625rem] uppercase tracking-[0.08em] text-mute">{label}</p>
      <p className="mt-1">
        <span
          className="font-mono text-sm"
          style={
            accent
              ? { color: ACCENT, backgroundColor: ACCENT_WASH, padding: '1px 4px', margin: '-1px -4px' }
              : { color: INK }
          }
        >
          {value}
        </span>
      </p>
      {hint && <p className="mt-1 text-[0.6875rem] leading-snug text-mute">{hint}</p>}
    </div>
  );
}

export function ExplainerToggle<Id extends string>({
  label,
  options,
  value,
  onChange,
}: {
  label: string;
  options: readonly { id: Id; label: string; accent?: boolean }[];
  value: Id;
  onChange: (id: Id) => void;
}) {
  return (
    <div className="flex flex-wrap items-center gap-3">
      <span className="font-mono text-[0.625rem] uppercase tracking-[0.08em] text-mute">
        {label}
      </span>
      <div className="inline-flex divide-x divide-line border border-line">
        {options.map((option) => {
          const active = option.id === value;
          return (
            <button
              key={option.id}
              type="button"
              onClick={() => onChange(option.id)}
              aria-pressed={active}
              className={cn(
                'px-2.5 py-1 font-mono text-[0.6875rem] transition-colors',
                'focus-visible:outline focus-visible:outline-1 focus-visible:-outline-offset-2 focus-visible:outline-[rgba(41,41,41,0.6)]',
                active && !option.accent && 'bg-[rgb(41,41,41)] text-[var(--bt-bg,#fff)]',
                !active && 'bg-bg text-mute hover:bg-panel',
              )}
              style={active && option.accent ? { backgroundColor: ACCENT_WASH, color: ACCENT } : undefined}
            >
              {option.label}
            </button>
          );
        })}
      </div>
    </div>
  );
}

export function ExplainerSlider({
  label,
  value,
  min,
  max,
  step,
  display,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  display: string;
  onChange: (value: number) => void;
  /** Accepted for backwards compatibility; the red styling is now the default. */
  accent?: boolean;
}) {
  const id = useId();
  const pct = ((value - min) / (max - min)) * 100;
  return (
    <div>
      <div className="mb-2 flex items-baseline justify-between gap-3">
        <label htmlFor={id} className="font-mono text-[0.625rem] uppercase tracking-[0.08em] text-mute">
          {label}
        </label>
        <span className="font-mono text-xs" style={{ color: INK }}>
          {display}
        </span>
      </div>
      <input
        id={id}
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
        style={{ ['--range-fill' as string]: `linear-gradient(to right, ${ACCENT} ${pct}%, rgba(41, 41, 41, 0.15) ${pct}%)` }}
        className={
          'h-4 w-full cursor-pointer appearance-none bg-transparent ' +
          'focus-visible:outline focus-visible:outline-1 focus-visible:outline-offset-4 focus-visible:outline-[rgba(41,41,41,0.6)] ' +
          '[&::-webkit-slider-runnable-track]:h-[2px] [&::-webkit-slider-runnable-track]:rounded-none [&::-webkit-slider-runnable-track]:[background:var(--range-fill)] ' +
          '[&::-webkit-slider-thumb]:-mt-[5px] [&::-webkit-slider-thumb]:h-3 [&::-webkit-slider-thumb]:w-3 [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:bg-[#d15168] ' +
          '[&::-moz-range-track]:h-[2px] [&::-moz-range-track]:bg-[rgba(41,41,41,0.15)] ' +
          '[&::-moz-range-progress]:h-[2px] [&::-moz-range-progress]:bg-[#d15168] ' +
          '[&::-moz-range-thumb]:h-3 [&::-moz-range-thumb]:w-3 [&::-moz-range-thumb]:rounded-full [&::-moz-range-thumb]:border-0 [&::-moz-range-thumb]:bg-[#d15168]'
        }
      />
    </div>
  );
}
