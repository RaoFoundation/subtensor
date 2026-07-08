'use client';

import React, {useState, useEffect} from 'react';
import {QueryClient, QueryClientProvider} from '@tanstack/react-query';
import {usePathname} from 'next/navigation';
import {Analytics} from '@vercel/analytics/react';
import clsx from 'clsx';
import {useHamburgerMenuStore} from './stores/useHamburgerMenuStore';
import {ThemeProvider, useTheme} from './contexts/ThemeContext';
import '@raofoundation/ui/styles/globals.css';
import styles from './layout.module.css';
import './global.css';

const PAGES_WITH_CUSTOM_OG_IMAGES = ['about', 'academia', 'charter', 'scan', 'whitepaper'];

function ThemedBody({children}: {children: React.ReactNode}) {
  const {theme} = useTheme();
  const pathname = usePathname();

  useEffect(() => {
    if (pathname.includes('/scan')) {
      document.documentElement.className = theme;
    } else {
      document.documentElement.className = 'light';
    }
  }, [pathname, theme]);

  return <>{children}</>;
}

export default function RootLayout({children}: {children: React.ReactNode}) {
  const isHamburgerMenuVisible = useHamburgerMenuStore((state) => state.isVisible);
  const [queryClient] = useState(() => new QueryClient());
  const pathname = usePathname();
  const pageName = pathname.split('/')[1];
  const ogImageFilename = PAGES_WITH_CUSTOM_OG_IMAGES.includes(pageName) ? pageName : 'default';

  return (
    <html lang='en' className={clsx('light', styles.html_container)}>
      <head>
        <title>Bittensor</title>
        <meta name='description' content='Internet-scale machine learning' />
        <meta name='twitter:card' content='summary_large_image' />
        <meta name='twitter:site' content='@bittensor_' />
        <meta name='twitter:creator' content='@opentensor' />
        <meta property='og:title' content='Bittensor' />
        <meta property='og:description' content='Internet-scale machine learning' />
        <meta property='og:image' content={'/images/og_thumbs/' + ogImageFilename + '.png'} />
        <link
          rel='stylesheet'
          href='https://cdn.jsdelivr.net/npm/katex@0.16.0/dist/katex.min.css'
        />
      </head>
      <body
        className={clsx(
          {[styles.no_scroll]: isHamburgerMenuVisible},
          styles.body_container,
          styles.body_container,
        )}
      >
        <ThemeProvider>
          <QueryClientProvider client={queryClient}>
            <ThemedBody>
              <main className={styles.main_container}>{children}</main>
            </ThemedBody>
          </QueryClientProvider>
        </ThemeProvider>
        <Analytics />
      </body>
    </html>
  );
}
