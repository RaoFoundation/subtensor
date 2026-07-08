'use client';

import type { ReactNode } from 'react';
import { cn } from '@/lib/cn';

export function ExplainerPanel({
  title,
  caption,
  children,
  className,
}: {
  title: string;
  caption?: string;
  children: ReactNode;
  className?: string;
}) {
  return (
    <figure className={cn('not-prose my-8 border border-line bg-panel', className)}>
      <figcaption className="border-b border-line px-4 py-2.5">
        <p className="bt-label text-mute">{title}</p>
        {caption && <p className="mt-1 text-[0.8125rem] leading-relaxed text-mute">{caption}</p>}
      </figcaption>
      <div className="p-4">{children}</div>
    </figure>
  );
}

export function ExplainerStat({
  label,
  value,
  hint,
}: {
  label: string;
  value: string;
  hint?: string;
}) {
  return (
    <div className="border border-line bg-bg px-3 py-2">
      <p className="bt-label text-mute">{label}</p>
      <p className="mt-1 font-mono text-sm">{value}</p>
      {hint && <p className="mt-0.5 text-[0.75rem] text-mute">{hint}</p>}
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
}) {
  return (
    <label className="block">
      <div className="mb-2 flex items-baseline justify-between gap-3">
        <span className="bt-label text-mute">{label}</span>
        <span className="font-mono text-xs">{display}</span>
      </div>
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
        className="w-full accent-[var(--bt-fg)]"
      />
    </label>
  );
}
