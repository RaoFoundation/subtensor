import React from 'react';
import type { Metadata } from 'next';
import { Header } from '@/app/components/Header/Header';
import { appName } from '@/lib/shared';
import '../docs/docs.css';
import './code.css';

export const metadata: Metadata = {
  title: {
    template: `%s — ${appName}`,
    default: appName,
  },
};

export default function CodeLayout({ children }: { children: React.ReactNode }) {
  return (
    <>
      <Header />
      <div className="bt-docs bt-codepage min-h-dvh">
        <main className="mx-auto w-full max-w-[70rem] px-5 py-10">{children}</main>
      </div>
    </>
  );
}
