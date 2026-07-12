import { getPageImage, source } from '@/lib/source';
import { notFound } from 'next/navigation';
import { ImageResponse } from 'next/og';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { appName } from '@/lib/shared';

export const revalidate = false;

// Static faces only — satori can't consume variable-weight TTFs.
const hafferPath = path.join(process.cwd(), 'src/app/fonts/Haffer-Medium.ttf');
const monoPath = path.join(process.cwd(), 'src/app/fonts/DMMono-Regular.ttf');

export async function GET(_req: Request, { params }: RouteContext<'/og/docs/[...slug]'>) {
  const { slug } = await params;
  const page = source.getPage(slug.slice(0, -1));
  if (!page) notFound();

  const [haffer, mono] = await Promise.all([
    readFile(hafferPath),
    readFile(monoPath),
  ]);

  return new ImageResponse(
    (
      <div
        style={{
          width: '100%',
          height: '100%',
          display: 'flex',
          flexDirection: 'column',
          justifyContent: 'space-between',
          background: '#ffffff',
          color: '#292929',
          padding: 72,
          fontFamily: 'Haffer',
        }}
      >
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 20,
            fontFamily: 'Mono',
            fontSize: 24,
            letterSpacing: '0.08em',
          }}
        >
          <svg width="36" height="36" fill="none" viewBox="0 0 21 23">
            <path
              fill="#292929"
              d="M12.53 17.783v-9.08a4.144 4.144 0 0 0-4.14-4.117v14.511a3.8 3.8 0 0 0 3.96 3.841 4.28 4.28 0 0 0 2.816-.816c-2.39-.253-2.635-1.693-2.635-4.339"
            />
            <path
              fill="#292929"
              d="M3.775.787A3.8 3.8 0 0 0 0 4.587h16.893a3.8 3.8 0 0 0 3.775-3.8z"
            />
          </svg>
          {appName.toUpperCase()}
        </div>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 24 }}>
          <div style={{ fontSize: 64, fontWeight: 500, letterSpacing: '-0.015em' }}>
            {page.data.title}
          </div>
          {page.data.description && (
            <div style={{ fontSize: 28, color: '#6e6e6e', lineHeight: 1.5 }}>
              {page.data.description}
            </div>
          )}
        </div>
      </div>
    ),
    {
      width: 1200,
      height: 630,
      fonts: [
        { name: 'Haffer', data: haffer, weight: 500 },
        { name: 'Mono', data: mono, weight: 400 },
      ],
    },
  );
}

export function generateStaticParams() {
  return source.getPages().map((page) => ({
    lang: page.locale,
    slug: getPageImage(page).segments,
  }));
}
