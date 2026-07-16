'use client';

import { useDocsSearch } from 'fumadocs-core/search/client';
import { useRouter } from 'next/navigation';
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import { CornerDownLeft, Search as SearchIcon, X } from 'lucide-react';
import { cn } from '@/lib/cn';

/** A registered focus target; returns false when it isn't visible (so the
    provider can fall through to another instance, e.g. mobile vs desktop). */
type Focuser = () => boolean;

const SearchContext = createContext<{
  register: (focus: Focuser) => () => void;
}>({ register: () => () => {} });

export function SearchProvider({ children }: { children: React.ReactNode }) {
  const focusers = useRef(new Set<Focuser>());

  const register = useCallback((focus: Focuser) => {
    focusers.current.add(focus);
    return () => {
      focusers.current.delete(focus);
    };
  }, []);

  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if ((event.metaKey || event.ctrlKey) && event.key === 'k') {
        event.preventDefault();
        for (const focus of focusers.current) {
          if (focus()) break;
        }
      }
    }
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  return (
    <SearchContext.Provider value={{ register }}>
      {children}
    </SearchContext.Provider>
  );
}

/* Result ordering: docs sections by importance. Relevance decides order
   within a section; the section decides order between them. */
const SECTIONS: { segment: string; label: string }[] = [
  { segment: '', label: 'Docs' },
  { segment: 'guides', label: 'Guides' },
  { segment: 'concepts', label: 'Concepts' },
  { segment: 'hyperparameters', label: 'Hyperparameters' },
  { segment: 'tx', label: 'Transactions' },
  { segment: 'query', label: 'Queries' },
  { segment: 'errors', label: 'Errors' },
  { segment: 'internals', label: 'Internals' },
];

function sectionOf(url: string): { rank: number; label: string } {
  const path = url.split('#')[0];
  const segments = path.split('/').filter(Boolean); // ["docs", "guides", ...]
  const segment = segments[1] ?? '';
  const rank = SECTIONS.findIndex((section) => section.segment === segment);
  // Unknown second segments are top-level pages (quickstart, cli, ...): Docs.
  if (rank === -1) return { rank: 0, label: SECTIONS[0].label };
  return { rank, label: SECTIONS[rank].label };
}

/** The row shape `useDocsSearch` yields (fumadocs SortedResult). */
type Result = {
  id: string;
  url: string;
  type: 'page' | 'heading' | 'text';
  content: string;
};

/** Group child matches under their page, sort groups by section importance
    (stable, so relevance still decides order within a section), then flatten
    back to rows, marking each row that starts a new section with its label. */
function sortResults(results: Result[]): { result: Result; section?: string }[] {
  const groups: Result[][] = [];
  for (const result of results) {
    // A match inside a pure-JSX line (e.g. a <Card /> block) cleans to
    // nothing; a blank row would just be confusing.
    if (result.type !== 'page' && cleanSnippet(result.content).trim() === '') continue;
    const current = groups[groups.length - 1];
    if (result.type === 'page' || !current) groups.push([result]);
    else current.push(result);
  }
  groups.sort((a, b) => sectionOf(a[0].url).rank - sectionOf(b[0].url).rank);

  const rows: { result: Result; section?: string }[] = [];
  let lastRank = -1;
  for (const group of groups) {
    const { rank, label } = sectionOf(group[0].url);
    for (const [index, result] of group.entries()) {
      rows.push(
        index === 0 && rank !== lastRank ? { result, section: label } : { result },
      );
    }
    lastRank = rank;
  }
  return rows;
}

/** The index stores raw markdown/MDX; strip everything that would render as
    noise (backticks, emphasis markers, JSX tags), keeping `<mark>` highlights. */
function cleanSnippet(text: string): string {
  return text
    .replaceAll('`', '')
    .replace(/<(?!\/?mark\b)\/?[A-Za-z][^>]*>/g, '')
    .replace(/\*\*([^*]+)\*\*/g, '$1')
    .replace(/__([^_]+)__/g, '$1');
}

/** Render a result snippet: `<mark>` highlights, markdown noise stripped. */
function Snippet({ text, bold }: { text: string; bold?: boolean }) {
  const parts = cleanSnippet(text).split(/<mark>(.*?)<\/mark>/g);
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

/** Inline sidebar search: type directly, results replace the nav (passed as
    children) until the query is cleared. ⌘K focuses the input. */
export function SidebarSearch({ children }: { children: React.ReactNode }) {
  const router = useRouter();
  const { search, setSearch, query } = useDocsSearch({ type: 'fetch' });
  const [selected, setSelected] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const { register } = useContext(SearchContext);

  useEffect(
    () =>
      register(() => {
        const input = inputRef.current;
        // offsetParent is null while hidden (e.g. the desktop rail on mobile).
        if (!input || input.offsetParent === null) return false;
        input.focus();
        return true;
      }),
    [register],
  );

  const rows = useMemo(
    () => sortResults(query.data !== 'empty' && query.data ? query.data : []),
    [query.data],
  );

  useEffect(() => {
    setSelected(0);
  }, [rows.length]);

  const active = search.length > 0;
  // While the debounced fetch is in flight (or hasn't started yet — data is
  // still the initial 'empty' sentinel), we can't say "No results." honestly.
  const pending = query.isLoading || (active && query.data === 'empty');

  function clear() {
    setSearch('');
  }

  function go(url: string) {
    clear();
    router.push(url);
  }

  function onKeyDown(event: React.KeyboardEvent) {
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      setSelected((value) => Math.min(value + 1, rows.length - 1));
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      setSelected((value) => Math.max(value - 1, 0));
    } else if (event.key === 'Enter' && rows[selected]) {
      event.preventDefault();
      go(rows[selected].result.url);
    } else if (event.key === 'Escape') {
      clear();
      inputRef.current?.blur();
    }
  }

  return (
    <>
      <div className="mb-4 flex items-center gap-2 border-b border-line ps-4 pe-2">
        <SearchIcon className="size-3.5 shrink-0 text-mute" />
        <input
          ref={inputRef}
          value={search}
          onChange={(event) => setSearch(event.target.value)}
          onKeyDown={onKeyDown}
          placeholder="Search the docs"
          className="bt-search-input w-full min-w-0 bg-transparent py-2 text-[0.8125rem] outline-none placeholder:text-mute"
        />
        {active ? (
          <button
            type="button"
            aria-label="Clear search"
            onClick={() => {
              clear();
              inputRef.current?.focus();
            }}
            className="p-1 text-mute hover:text-fg"
          >
            <X className="size-3" />
          </button>
        ) : (
          <kbd className="bt-label shrink-0 font-mono text-[10px] text-mute max-sm:hidden">
            ⌘K
          </kbd>
        )}
      </div>
      {active ? (
        <div>
          {rows.length === 0 && (
            <p className="ps-4 py-2 text-[0.8125rem] text-mute">
              {pending ? 'Searching…' : 'No results.'}
            </p>
          )}
          {rows.map(({ result, section }, index) => (
            <div key={result.id}>
              {section !== undefined && (
                <p className={cn('bt-label mb-1 ps-4 text-mute', index === 0 ? 'mt-1' : 'mt-4')}>
                  {section}
                </p>
              )}
              <button
                type="button"
                onClick={() => go(result.url)}
                onMouseMove={() => setSelected(index)}
                className={cn(
                  'flex w-full items-center gap-2 border-s py-1.5 ps-4 pe-2 text-start text-[0.8125rem] leading-snug',
                  result.type === 'page'
                    ? 'border-transparent text-fg'
                    : 'border-transparent ps-7 text-mute',
                  index === selected && 'border-fg bg-hover text-fg',
                )}
              >
                <Snippet text={result.content} bold={result.type === 'page'} />
                {index === selected && (
                  <CornerDownLeft className="size-3 shrink-0 text-mute" />
                )}
              </button>
            </div>
          ))}
        </div>
      ) : (
        children
      )}
    </>
  );
}
