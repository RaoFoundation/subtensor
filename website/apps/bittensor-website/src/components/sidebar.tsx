'use client';

import Link from 'next/link';
import { usePathname } from 'next/navigation';
import {
  createContext,
  useContext,
  useEffect,
  useState,
} from 'react';
import { ChevronRight, Menu, Search as SearchIcon, X } from 'lucide-react';
import type { TreeNode } from '@/lib/tree';
import { cn } from '@/lib/cn';
import { SidebarSearch, useSearchController } from './search';

/* The bittensor.com header is 88px tall on desktop (32px padding + 24px logo). */
const HEADER_OFFSET = 'top-[88px] h-[calc(100dvh-88px)]';

/* Drawer state is shared so the trigger can sit in the header while the
   drawer itself belongs to the sidebar. */
const DrawerContext = createContext<{
  open: boolean;
  setOpen: (value: boolean) => void;
}>({ open: false, setOpen: () => {} });

export function SidebarProvider({ children }: { children: React.ReactNode }) {
  const [open, setOpen] = useState(false);
  const pathname = usePathname();
  const { registerMobileOpener } = useSearchController();

  useEffect(() => {
    setOpen(false);
  }, [pathname]);

  useEffect(
    () => registerMobileOpener(() => setOpen(true)),
    [registerMobileOpener],
  );

  return (
    <DrawerContext.Provider value={{ open, setOpen }}>
      {children}
    </DrawerContext.Provider>
  );
}

export function SidebarTrigger() {
  const { setOpen } = useContext(DrawerContext);
  return (
    <button
      type="button"
      aria-label="Open navigation"
      onClick={() => setOpen(true)}
      className="p-2 -ms-2 text-mute hover:text-fg md:hidden"
    >
      <Menu className="size-4" />
    </button>
  );
}

/** Mobile header search button: opens the drawer and focuses the input. */
export function SidebarSearchTrigger() {
  const { open } = useSearchController();
  return (
    <button
      type="button"
      aria-label="Search"
      onClick={() => open()}
      className="p-2 text-mute hover:text-fg md:hidden"
    >
      <SearchIcon className="size-3.5" />
    </button>
  );
}

function folderContains(node: TreeNode, pathname: string): boolean {
  if (node.type === 'page') return node.url === pathname;
  if (node.type === 'folder') {
    if (node.url === pathname) return true;
    return node.children.some((child) => folderContains(child, pathname));
  }
  return false;
}

function PageLink({
  node,
  pathname,
}: {
  node: Extract<TreeNode, { type: 'page' }>;
  pathname: string;
}) {
  const active = node.url === pathname;
  return (
    <Link
      href={node.url}
      className={cn(
        'block border-s py-1.5 ps-4 text-[0.8125rem] leading-snug transition-colors',
        active
          ? 'border-fg text-fg font-medium'
          : 'border-transparent text-mute hover:text-fg',
      )}
    >
      {node.name}
    </Link>
  );
}

function Folder({
  node,
  pathname,
}: {
  node: Extract<TreeNode, { type: 'folder' }>;
  pathname: string;
}) {
  const containsActive = folderContains(node, pathname);
  const [open, setOpen] = useState(containsActive);

  useEffect(() => {
    if (containsActive) setOpen(true);
  }, [containsActive]);

  const rowClass = cn(
    'flex w-full items-center gap-1.5 border-s py-1.5 ps-4 text-start text-[0.8125rem] leading-snug transition-colors',
    pathname === node.url
      ? 'border-fg text-fg font-medium'
      : cn('border-transparent', containsActive ? 'text-fg' : 'text-mute hover:text-fg'),
  );

  const chevron = (
    <button
      type="button"
      aria-label={open ? `Collapse ${node.name}` : `Expand ${node.name}`}
      onClick={(event) => {
        event.preventDefault();
        event.stopPropagation();
        setOpen((value) => !value);
      }}
      className="p-1 -m-1 me-1 text-mute hover:text-fg"
    >
      <ChevronRight
        className={cn('size-3 shrink-0 transition-transform', open && 'rotate-90')}
      />
    </button>
  );

  return (
    <div>
      {node.url ? (
        <Link href={node.url} className={rowClass} onClick={() => setOpen(true)}>
          <span className="flex-1">{node.name}</span>
          {chevron}
        </Link>
      ) : (
        <button
          type="button"
          onClick={() => setOpen((value) => !value)}
          className={rowClass}
        >
          <span className="flex-1">{node.name}</span>
          <ChevronRight
            className={cn('size-3 shrink-0 transition-transform', open && 'rotate-90')}
          />
        </button>
      )}
      {open && (
        <div className="ms-4 border-s border-line">
          <Tree nodes={node.children} pathname={pathname} />
        </div>
      )}
    </div>
  );
}

function Tree({ nodes, pathname }: { nodes: TreeNode[]; pathname: string }) {
  return (
    <>
      {nodes.map((node, index) => {
        if (node.type === 'separator') {
          return (
            <p
              key={index}
              className={cn(
                'bt-label mb-2 ps-4 text-mute',
                index === 0 ? 'mt-2' : 'mt-8',
              )}
            >
              {node.name}
            </p>
          );
        }
        if (node.type === 'folder') {
          return <Folder key={node.url ?? node.name} node={node} pathname={pathname} />;
        }
        return <PageLink key={node.url} node={node} pathname={pathname} />;
      })}
    </>
  );
}

export function Sidebar({ tree }: { tree: TreeNode[] }) {
  const pathname = usePathname();
  const { open, setOpen } = useContext(DrawerContext);

  return (
    <>
      {/* Desktop rail */}
      <aside
        className={cn(
          'max-md:hidden sticky w-64 shrink-0 overflow-y-auto bt-scroll py-6 pe-4',
          HEADER_OFFSET,
        )}
      >
        <SidebarSearch>
          <nav>
            <Tree nodes={tree} pathname={pathname} />
          </nav>
        </SidebarSearch>
      </aside>

      {/* Mobile drawer; z-index sits above the sticky site header (z-index 100). */}
      {open && (
        <div className="fixed inset-0 z-[110] md:hidden">
          <div
            className="absolute inset-0 bg-fg/20"
            onClick={() => setOpen(false)}
          />
          <div className="absolute inset-y-0 start-0 w-72 overflow-y-auto bt-scroll bg-bg p-4">
            <button
              type="button"
              aria-label="Close navigation"
              onClick={() => setOpen(false)}
              className="mb-4 p-2 text-mute hover:text-fg"
            >
              <X className="size-4" />
            </button>
            <SidebarSearch>
              <Tree nodes={tree} pathname={pathname} />
            </SidebarSearch>
          </div>
        </div>
      )}
    </>
  );
}
