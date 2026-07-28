import React from 'react';
import type {Metadata, Viewport} from 'next';
import {Analytics} from '@vercel/analytics/react';
import clsx from 'clsx';
import {siteUrl} from '@/lib/shared';
import {BodyScrollLock} from './components/BodyScrollLock';
import '@raofoundation/ui/styles/globals.css';
import 'katex/dist/katex.min.css';
import styles from './layout.module.css';
import './global.css';

const siteDescription =
  'Bittensor is an open, decentralized network where independent subnets produce ' +
  'digital commodities — compute, inference, storage, prediction — and the chain ' +
  'pays contributors in TAO.';

export const metadata: Metadata = {
  metadataBase: new URL(siteUrl),
  title: {
    default: 'Bittensor',
    template: '%s — Bittensor',
  },
  description: siteDescription,
  openGraph: {
    siteName: 'Bittensor',
    type: 'website',
    title: 'Bittensor',
    description: siteDescription,
    images: '/images/og_thumbs/default.png',
  },
  twitter: {
    card: 'summary_large_image',
    site: '@bittensor',
    creator: '@bittensor',
  },
};

export const viewport: Viewport = {
  width: 'device-width',
  initialScale: 1,
};

const organizationJsonLd = {
  '@context': 'https://schema.org',
  '@type': 'Organization',
  name: 'Bittensor',
  url: siteUrl,
  sameAs: ['https://x.com/bittensor', 'https://github.com/RaoFoundation'],
};

export default function RootLayout({children}: {children: React.ReactNode}) {
  return (
    <html lang='en' className={clsx('light', styles.html_container)}>
      <body className={styles.body_container}>
        <script
          type='application/ld+json'
          dangerouslySetInnerHTML={{__html: JSON.stringify(organizationJsonLd)}}
        />
        <BodyScrollLock />
        <main className={styles.main_container}>{children}</main>
        <Analytics />
      </body>
    </html>
  );
}
