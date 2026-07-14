'use client';

import { useDocsSearch } from 'fumadocs-core/search/client';
import { useRouter } from 'next/navigation';
import {
  createContext,
  useContext,
  useEffect,
  useRef,
  useState,
} from 'react';
import { CornerDownLeft, Search as SearchIcon } from 'lucide-react';
import { cn } from '@/lib/cn';

const SearchContext = createContext<{ open: () => void }>({ open: () => {} });

export function useSearch() {
  return useContext(SearchContext);
}

export function SearchProvider({ children }: { children: React.ReactNode }) {
  const [open, setOpen] = useState(false);

  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if ((event.metaKey || event.ctrlKey) && event.key === 'k') {
        event.preventDefault();
        setOpen((value) => !value);
      }
      if (event.key === 'Escape') setOpen(false);
    }
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  return (
    <SearchContext.Provider value={{ open: () => setOpen(true) }}>
      {children}
      {open && <SearchDialog onClose={() => setOpen(false)} />}
    </SearchContext.Provider>
  );
}

export function SearchTrigger() {
  const { open } = useSearch();
  return (
    <button
      type="button"
      aria-label="Search"
      onClick={open}
      className="bt-label flex items-center gap-2 py-1.5 text-mute hover:text-fg transition-colors"
    >
      <SearchIcon className="size-3.5" />
      <kbd className="max-sm:hidden font-mono text-[10px] tracking-normal normal-case">
        ⌘K
      </kbd>
    </button>
  );
}

/** Render a result snippet: `<mark>` highlights, backticks stripped. */
function Snippet({ text, bold }: { text: string; bold?: boolean }) {
  const clean = text.replaceAll('`', '');
  const parts = clean.split(/<mark>(.*?)<\/mark>/g);
  return (
    <span className={cn('flex-1 truncate', bold && 'font-medium')}>
      {parts.map((part, index) =>
        index % 2 === 1 ? (
          <span key={index} className="underline underline-offset-2">
            {part}
          </span>
        ) : (
          part
        ),
      )}
    </span>
  );
}

function SearchDialog({ onClose }: { onClose: () => void }) {
  const router = useRouter();
  const { search, setSearch, query } = useDocsSearch({ type: 'fetch' });
  const [selected, setSelected] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const results = query.data !== 'empty' && query.data ? query.data : [];

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    setSelected(0);
  }, [results.length]);

  function go(url: string) {
    onClose();
    router.push(url);
  }

  function onKeyDown(event: React.KeyboardEvent) {
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      setSelected((value) => Math.min(value + 1, results.length - 1));
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      setSelected((value) => Math.max(value - 1, 0));
    } else if (event.key === 'Enter' && results[selected]) {
      event.preventDefault();
      go(results[selected].url);
    }
  }

  return (
    <div
      className="fixed inset-0 z-[120] bg-fg/20 flex items-start justify-center pt-[15vh] px-4"
      onClick={onClose}
    >
      <div
        className="w-full max-w-lg bg-bg border border-line shadow-xl"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="flex items-center gap-3 border-b border-line px-4">
          <SearchIcon className="size-3.5 text-mute shrink-0" />
          <input
            ref={inputRef}
            value={search}
            onChange={(event) => setSearch(event.target.value)}
            onKeyDown={onKeyDown}
            placeholder="Search the docs"
            className="w-full bg-transparent py-3.5 text-sm outline-none placeholder:text-mute"
          />
          <kbd className="bt-label text-mute shrink-0">esc</kbd>
        </div>
        <div className="max-h-[50vh] overflow-y-auto bt-scroll">
          {search.length > 0 && results.length === 0 && !query.isLoading && (
            <p className="px-4 py-6 text-sm text-mute">No results.</p>
          )}
          {results.map((result, index) => (
            <button
              key={result.id}
              type="button"
              onClick={() => go(result.url)}
              onMouseMove={() => setSelected(index)}
              className={cn(
                'flex w-full items-center gap-3 px-4 py-2.5 text-start text-sm',
                result.type !== 'page' && 'ps-8 text-mute',
                index === selected && 'bg-hover text-fg',
              )}
            >
              <Snippet text={result.content} bold={result.type === 'page'} />
              {result.type === 'page' &&
                result.breadcrumbs &&
                result.breadcrumbs.length > 1 && (
                  <span className="bt-label shrink-0 text-mute">
                    {result.breadcrumbs.slice(1).join(' / ')}
                  </span>
                )}
              {index === selected && (
                <CornerDownLeft className="size-3 shrink-0 text-mute" />
              )}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
