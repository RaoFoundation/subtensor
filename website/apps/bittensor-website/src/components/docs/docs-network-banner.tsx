import {getDocsNetwork} from '@/lib/docs-network';

export function DocsNetworkBanner() {
  const info = getDocsNetwork();
  if (!info) {
    return null;
  }

  return (
    <aside
      className='border-b border-fg bg-panel px-5 py-3 text-[0.8125rem] leading-relaxed text-fg'
      role='status'
    >
      <div className='mx-auto flex w-full max-w-[90rem] flex-col gap-2 md:flex-row md:items-baseline md:justify-between md:gap-6'>
        <p>
          <span className='font-mono text-[0.6875rem] uppercase tracking-[0.08em]'>
            {info.label} docs
          </span>
          <span className='text-mute'> — </span>
          These docs match the code on {info.label}. {info.installHint} Point{' '}
          <code className='font-mono text-[0.8125em]'>btcli</code> / the SDK at{' '}
          <code className='font-mono text-[0.8125em]'>{info.chainNetwork}</code>.
        </p>
        <code className='shrink-0 break-all font-mono text-[0.75rem] text-mute md:max-w-[36rem] md:text-end'>
          {info.installCommand}
        </code>
      </div>
    </aside>
  );
}
