import Link from 'next/link';
import { notFound } from 'next/navigation';
import type { Metadata } from 'next';
import { ArrowUpRight } from 'lucide-react';
import { createHighlighter, type Highlighter } from 'shiki';
import {
  buildCommit,
  codeDirs,
  codeFiles,
  getCodeFile,
  isCodeDir,
  listCodeDir,
  readCodeFile,
} from '@/lib/code';
import { btDark, btLight } from '@/lib/shiki-themes';
import { chainRepoUrl, codeRoute, siteUrl } from '@/lib/shared';

// The corpus is enumerated at build time; anything outside it is a 404, so no
// request ever touches the filesystem.
export const dynamicParams = false;

export async function generateStaticParams() {
  return [
    { slug: [] },
    ...codeDirs().map((dir) => ({ slug: dir.split('/') })),
    ...codeFiles().map((file) => ({ slug: file.path.split('/') })),
  ];
}

// One highlighter for the whole build (generateStaticParams renders pages in
// parallel workers; the promise cache keeps each worker to a single instance).
const highlighterCache = globalThis as { __btHighlighter?: Promise<Highlighter> };

function getHighlighter(): Promise<Highlighter> {
  highlighterCache.__btHighlighter ??= createHighlighter({
    langs: ['rust'],
    themes: [btLight, btDark],
  });
  return highlighterCache.__btHighlighter;
}

function formatCount(n: number): string {
  return n.toLocaleString('en-US');
}

function Breadcrumbs({ segments }: { segments: string[] }) {
  return (
    <p className="bt-label mb-4 text-mute">
      <Link href={codeRoute} className="hover:text-fg transition-colors">
        code
      </Link>
      {segments.map((segment, index) => {
        const isLast = index === segments.length - 1;
        const href = `${codeRoute}/${segments.slice(0, index + 1).join('/')}`;
        return (
          <span key={href}>
            <span className="mx-2">/</span>
            {isLast ? (
              <span className="text-fg">{segment}</span>
            ) : (
              <Link href={href} className="hover:text-fg transition-colors">
                {segment}
              </Link>
            )}
          </span>
        );
      })}
    </p>
  );
}

function DirectoryPage({ dirPath }: { dirPath: string }) {
  const listing = listCodeDir(dirPath);
  const base = dirPath === '' ? codeRoute : `${codeRoute}/${dirPath}`;
  const commit = buildCommit();

  return (
    <div className="mx-auto max-w-[44rem]">
      <Breadcrumbs segments={dirPath === '' ? [] : dirPath.split('/')} />
      <h1 className="text-[1.625rem] font-medium tracking-[-0.015em]">
        {dirPath === '' ? 'Chain source' : dirPath.split('/').pop()}
      </h1>
      {dirPath === '' && (
        <p className="mt-3 text-[0.9375rem] font-light leading-relaxed text-mute">
          The Rust that runs on the chain — the pallets, the runtime that
          assembles them, and their shared primitives — as of the commit this
          site was built from. Tests, mocks, and benchmarks are omitted. Raw
          text lives under <code className="font-mono text-[0.8125em]">/code/raw/…</code>;
          the machine-readable index is{' '}
          <a href="/code/index.json" className="underline underline-offset-2">
            /code/index.json
          </a>
          .
        </p>
      )}
      <div className="mt-8 border-t border-line">
        {listing.dirs.map((dir) => (
          <Link
            key={dir.name}
            href={`${base}/${dir.name}`}
            className="flex items-baseline justify-between gap-4 border-b border-line py-2.5 hover:bg-hover transition-colors"
          >
            <span className="font-mono text-[0.8125rem]">{dir.name}/</span>
            <span className="bt-label text-mute">
              {formatCount(dir.files)} {dir.files === 1 ? 'file' : 'files'} ·{' '}
              {formatCount(dir.lines)} lines
            </span>
          </Link>
        ))}
        {listing.files.map((file) => (
          <Link
            key={file.path}
            href={`${codeRoute}/${file.path}`}
            className="flex items-baseline justify-between gap-4 border-b border-line py-2.5 hover:bg-hover transition-colors"
          >
            <span className="font-mono text-[0.8125rem]">
              {file.path.split('/').pop()}
            </span>
            <span className="bt-label text-mute">{formatCount(file.lines)} lines</span>
          </Link>
        ))}
      </div>
      {dirPath === '' && commit && (
        <p className="bt-label mt-6 text-mute">
          built from{' '}
          <a
            href={`${chainRepoUrl}/tree/${commit}`}
            target="_blank"
            rel="noreferrer"
            className="hover:text-fg transition-colors"
          >
            {commit.slice(0, 10)}
          </a>
        </p>
      )}
    </div>
  );
}

async function FilePage({ filePath }: { filePath: string }) {
  const file = getCodeFile(filePath);
  if (!file) notFound();

  const content = readCodeFile(filePath);
  const commit = buildCommit();
  const highlighter = await getHighlighter();
  const html = highlighter.codeToHtml(content, {
    lang: 'rust',
    themes: { light: 'bt-light', dark: 'bt-dark' },
    defaultColor: 'light',
    transformers: [
      {
        line(node, line) {
          node.properties.id = `L${line}`;
          node.children.unshift({
            type: 'element',
            tagName: 'a',
            properties: { href: `#L${line}`, className: 'bt-ln', 'aria-hidden': 'true', tabIndex: -1 },
            children: [],
          });
        },
      },
    ],
  });

  const segments = filePath.split('/');
  const githubUrl = commit
    ? `${chainRepoUrl}/blob/${commit}/${filePath}`
    : `${chainRepoUrl}/blob/main/${filePath}`;

  return (
    <div>
      <Breadcrumbs segments={segments} />
      <div className="flex flex-wrap items-baseline gap-x-5 gap-y-2">
        <h1 className="font-mono text-[1.125rem] font-medium">
          {segments[segments.length - 1]}
        </h1>
        <span className="bt-label text-mute">
          {formatCount(file.lines)} lines · {formatCount(file.bytes)} bytes
          {commit && <> · {commit.slice(0, 10)}</>}
        </span>
        <span className="flex items-baseline gap-4">
          <a
            href={`${codeRoute}/raw/${filePath}`}
            className="bt-label flex items-center gap-1 text-mute hover:text-fg transition-colors"
          >
            Raw
            <ArrowUpRight className="size-3" />
          </a>
          <a
            href={githubUrl}
            target="_blank"
            rel="noreferrer"
            className="bt-label flex items-center gap-1 text-mute hover:text-fg transition-colors"
          >
            GitHub
            <ArrowUpRight className="size-3" />
          </a>
        </span>
      </div>
      <div
        className="bt-code bt-scroll mt-6"
        dangerouslySetInnerHTML={{ __html: html }}
      />
    </div>
  );
}

export default async function Page(props: PageProps<'/code/[[...slug]]'>) {
  const params = await props.params;
  const slug = params.slug ?? [];
  const joined = slug.join('/');

  if (joined === '' || isCodeDir(joined)) return <DirectoryPage dirPath={joined} />;
  return <FilePage filePath={joined} />;
}

export async function generateMetadata(
  props: PageProps<'/code/[[...slug]]'>,
): Promise<Metadata> {
  const params = await props.params;
  const joined = (params.slug ?? []).join('/');
  const title = joined === '' ? 'Chain source' : joined;
  const description =
    joined === ''
      ? 'The Rust source that runs on the Bittensor chain, browsable file by file.'
      : `On-chain source: ${joined} at the commit this site was built from.`;

  return {
    title,
    description,
    alternates: {
      canonical: joined === '' ? codeRoute : `${codeRoute}/${joined}`,
    },
    openGraph: {
      type: 'article',
      title,
      description,
      url: `${siteUrl}${joined === '' ? codeRoute : `${codeRoute}/${joined}`}`,
    },
  };
}
