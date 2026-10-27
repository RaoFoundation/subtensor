'use client';

import { useEffect } from 'react';

// #L12 targets one line, #L12-L34 a range (order-insensitive). Single-line
// anchors also work without JS via the CSS :target rule; this component adds
// the range case, which has no real element to :target.
const HASH_RE = /^#L(\d+)(?:-L(\d+))?$/;

function parseHash(hash: string): [number, number] | null {
  const match = HASH_RE.exec(hash);
  if (!match) return null;
  const a = Number(match[1]);
  const b = match[2] ? Number(match[2]) : a;
  return a <= b ? [a, b] : [b, a];
}

/**
 * Highlights the line or line range addressed by the URL hash in the /code
 * source view, and lets readers author range links by shift-clicking a second
 * line number (GitHub-style).
 */
export function LineRangeHighlight() {
  useEffect(() => {
    const apply = (scroll: boolean) => {
      for (const el of document.querySelectorAll('.bt-code .line.bt-hl')) {
        el.classList.remove('bt-hl');
      }
      const range = parseHash(window.location.hash);
      if (!range) return;
      const [start, end] = range;
      for (let n = start; n <= end; n++) {
        document.getElementById(`L${n}`)?.classList.add('bt-hl');
      }
      // Ranges have no element with the hash's id, so the browser never
      // scrolls to them on its own. scrollIntoView honors the lines'
      // scroll-margin-top.
      if (scroll) document.getElementById(`L${start}`)?.scrollIntoView();
    };

    // The browser only scrolls by itself when an element carries the hash's
    // exact id — true for #L12, never for #L12-L34.
    const onHashChange = () =>
      apply(!document.getElementById(window.location.hash.slice(1)));

    // Shift-click on a line-number gutter extends the current anchor into a
    // range ending (or starting) at the clicked line.
    const onClick = (event: MouseEvent) => {
      if (!event.shiftKey) return;
      const gutter = (event.target as Element).closest('a.bt-ln');
      if (!gutter) return;
      const line = gutter.parentElement && parseHash(`#${gutter.parentElement.id}`);
      const current = parseHash(window.location.hash);
      if (!line || !current) return;
      event.preventDefault();
      const [clicked] = line;
      const [start, end] =
        clicked < current[0] ? [clicked, current[1]] : [current[0], clicked];
      history.replaceState(null, '', start === end ? `#L${start}` : `#L${start}-L${end}`);
      apply(false);
    };

    apply(true);
    window.addEventListener('hashchange', onHashChange);
    document.addEventListener('click', onClick);
    return () => {
      window.removeEventListener('hashchange', onHashChange);
      document.removeEventListener('click', onClick);
    };
  }, []);

  return null;
}
