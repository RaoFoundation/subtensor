'use client';

import { Check, Copy } from 'lucide-react';
import { useRef, useState } from 'react';
import { cn } from '@/lib/cn';

function useCopied() {
  const [copied, setCopied] = useState(false);
  const timeout = useRef<ReturnType<typeof setTimeout> | null>(null);
  return {
    copied,
    flash() {
      setCopied(true);
      if (timeout.current) clearTimeout(timeout.current);
      timeout.current = setTimeout(() => setCopied(false), 1500);
    },
  };
}

/** Copy button for code blocks; reads the sibling <pre>'s text. */
export function CopyCodeButton() {
  const { copied, flash } = useCopied();
  const ref = useRef<HTMLButtonElement>(null);

  return (
    <button
      ref={ref}
      type="button"
      aria-label="Copy code"
      className="bt-copy p-1.5 border border-line bg-bg text-mute hover:text-fg transition-colors"
      onClick={async () => {
        const pre = ref.current?.closest('.bt-codeblock')?.querySelector('pre');
        if (!pre?.textContent) return;
        await navigator.clipboard.writeText(pre.textContent);
        flash();
      }}
    >
      {copied ? <Check className="size-3" /> : <Copy className="size-3" />}
    </button>
  );
}

/** Compact EVM address that copies the complete 20-byte value. */
export function EvmAddress({ address }: { address: string }) {
  const { copied, flash } = useCopied();
  const shortAddress = `${address.slice(0, 3)}...${address.slice(-4)}`;

  return (
    <button
      type="button"
      aria-label={`Copy address ${address}`}
      title={address}
      className="inline-flex items-center gap-1 font-mono text-[0.8125rem] text-fg hover:text-mute transition-colors"
      onClick={async () => {
        await navigator.clipboard.writeText(address);
        flash();
      }}
    >
      {shortAddress}
      {copied ? <Check className="size-3" /> : <Copy className="size-3" />}
    </button>
  );
}

/** "Copy Markdown" — fetches the page's raw markdown and copies it. */
export function CopyMarkdownButton({
  markdownUrl,
  className,
}: {
  markdownUrl: string;
  className?: string;
}) {
  const { copied, flash } = useCopied();

  return (
    <button
      type="button"
      className={cn(
        'bt-label flex items-center gap-1.5 text-mute hover:text-fg transition-colors',
        className,
      )}
      onClick={async () => {
        const response = await fetch(markdownUrl);
        await navigator.clipboard.writeText(await response.text());
        flash();
      }}
    >
      {copied ? <Check className="size-3" /> : <Copy className="size-3" />}
      Copy Markdown
    </button>
  );
}
