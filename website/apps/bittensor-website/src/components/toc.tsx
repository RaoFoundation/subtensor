'use client';

import { useEffect, useState } from 'react';
import { cn } from '@/lib/cn';

export interface TocEntry {
  title: React.ReactNode;
  url: string;
  depth: number;
}

export function Toc({ items }: { items: TocEntry[] }) {
  const [active, setActive] = useState<string | null>(null);

  useEffect(() => {
    const headings = items
      .map((item) => document.getElementById(item.url.slice(1)))
      .filter((element): element is HTMLElement => element !== null);
    if (headings.length === 0) return;

    const visible = new Set<string>();
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (entry.isIntersecting) visible.add(entry.target.id);
          else visible.delete(entry.target.id);
        }
        // Highlight the first visible heading; when none are visible (between
        // sections), keep the last one that scrolled past the top.
        if (visible.size > 0) {
          const first = headings.find((heading) => visible.has(heading.id));
          if (first) setActive(first.id);
        } else {
          const above = headings.filter(
            (heading) => heading.getBoundingClientRect().top < 100,
          );
          if (above.length > 0) setActive(above[above.length - 1].id);
        }
      },
      { rootMargin: '-80px 0px -60% 0px' },
    );
    for (const heading of headings) observer.observe(heading);
    return () => observer.disconnect();
  }, [items]);

  if (items.length === 0) return null;

  return (
    <nav className="max-xl:hidden sticky top-[88px] h-[calc(100dvh-88px)] w-56 shrink-0 overflow-y-auto bt-scroll py-10 ps-8">
      <p className="bt-label mb-4 text-mute">On this page</p>
      <ul className="space-y-0">
        {items.map((item) => (
          <li key={item.url}>
            <a
              href={item.url}
              className={cn(
                'block py-1 text-[0.8125rem] leading-snug transition-colors',
                item.depth > 2 && 'ps-3',
                active === item.url.slice(1)
                  ? 'text-fg'
                  : 'text-mute hover:text-fg',
              )}
            >
              {item.title}
            </a>
          </li>
        ))}
      </ul>
    </nav>
  );
}
