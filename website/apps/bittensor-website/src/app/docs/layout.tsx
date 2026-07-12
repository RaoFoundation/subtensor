import React from 'react';
import type {Metadata} from 'next';
import {Header} from '@/app/components/Header/Header';
import {SearchProvider, SearchTrigger} from '@/components/search';
import {Sidebar, SidebarProvider, SidebarTrigger} from '@/components/sidebar';
import {source} from '@/lib/source';
import {serializeTree} from '@/lib/tree';
import {appName} from '@/lib/shared';
import './docs.css';

export const metadata: Metadata = {
  title: {
    template: `%s — ${appName}`,
    default: appName,
  },
};

export default function DocsLayout({children}: {children: React.ReactNode}) {
  const tree = serializeTree(source.getPageTree());

  return (
    <>
      <Header />
      <div className='bt-docs min-h-dvh'>
        <SearchProvider>
          <SidebarProvider>
            <div className='flex items-center gap-5 px-5 py-2 md:hidden'>
              <SidebarTrigger />
              <SearchTrigger />
            </div>
            <div className='mx-auto flex w-full max-w-[90rem] px-5'>
              <Sidebar tree={tree} />
              {children}
            </div>
          </SidebarProvider>
        </SearchProvider>
      </div>
    </>
  );
}
