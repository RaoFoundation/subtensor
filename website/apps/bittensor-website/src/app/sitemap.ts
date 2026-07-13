import type {MetadataRoute} from 'next';
import {source} from '@/lib/source';
import {siteUrl} from '@/lib/shared';

const staticRoutes = [
  '/',
  '/about',
  '/academia',
  '/charter',
  '/dtao-whitepaper',
  '/explore',
  '/intro',
  '/releases/v430-upgrade',
  '/wallet',
  '/whitepaper',
];

export default function sitemap(): MetadataRoute.Sitemap {
  const pages = [
    ...staticRoutes,
    ...source.getPages().map((page) => page.url),
  ];

  return pages.map((path) => ({
    url: `${siteUrl}${path === '/' ? '' : path}`,
    changeFrequency: 'weekly',
    priority: path === '/' || path === '/docs' ? 1 : 0.7,
  }));
}
