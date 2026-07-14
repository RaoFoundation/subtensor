import { getPageImage, getPageMarkdownUrl, source } from '@/lib/source';
import { notFound } from 'next/navigation';
import type { Metadata } from 'next';
import { getBreadcrumbItems } from 'fumadocs-core/breadcrumb';
import { ArrowUpRight } from 'lucide-react';
import { getMDXComponents } from '@/components/mdx';
import { Toc, type TocEntry } from '@/components/toc';
import { CopyMarkdownButton } from '@/components/copy';
import { siteUrl } from '@/lib/shared';

// Reference titles are bare slugs ("transfer", "balance"); qualify them so the
// <title> carries the terms people actually search for.
const SECTION_TITLE_SUFFIX: Record<string, string> = {
  tx: 'SDK transaction',
  query: 'SDK query',
};

function pageJsonLd(page: NonNullable<ReturnType<typeof source.getPage>>) {
  const crumbs = getBreadcrumbItems(page.url, source.getPageTree());

  return [
    {
      '@context': 'https://schema.org',
      '@type': 'TechArticle',
      headline: page.data.title,
      description: page.data.description,
      url: `${siteUrl}${page.url}`,
      image: `${siteUrl}${getPageImage(page).url}`,
      author: { '@type': 'Organization', name: 'Bittensor' },
    },
    {
      '@context': 'https://schema.org',
      '@type': 'BreadcrumbList',
      itemListElement: [
        { '@type': 'ListItem', position: 1, name: 'Docs', item: `${siteUrl}/docs` },
        ...crumbs.map((crumb, index) => ({
          '@type': 'ListItem',
          position: index + 2,
          name: crumb.name,
          ...(crumb.url ? { item: `${siteUrl}${crumb.url}` } : {}),
        })),
      ],
    },
  ];
}

export default async function Page(props: PageProps<'/docs/[[...slug]]'>) {
  const params = await props.params;
  const page = source.getPage(params.slug);
  if (!page) notFound();

  const MDX = page.data.body;
  const markdownUrl = getPageMarkdownUrl(page).url;
  const crumbs = getBreadcrumbItems(page.url, source.getPageTree());

  return (
    <>
      <script
        type="application/ld+json"
        dangerouslySetInnerHTML={{ __html: JSON.stringify(pageJsonLd(page)) }}
      />
      <main className="min-w-0 flex-1 py-10 md:ps-10">
        <div className="mx-auto max-w-[44rem]">
          {crumbs.length > 0 && (
            <p className="bt-label mb-4 text-mute">
              {crumbs.map((crumb, index) => (
                <span key={index}>
                  {index > 0 && <span className="mx-2">/</span>}
                  {crumb.name}
                </span>
              ))}
            </p>
          )}
          <h1 className="text-[1.625rem] font-medium tracking-[-0.015em]">
            {page.data.title}
          </h1>
          {page.data.description && (
            <p className="mt-3 text-[0.9375rem] font-light leading-relaxed text-mute">
              {page.data.description}
            </p>
          )}
          <div className="mt-5 flex items-center gap-5">
            <CopyMarkdownButton markdownUrl={markdownUrl} />
            <a
              href={markdownUrl}
              target="_blank"
              rel="noreferrer"
              className="bt-label flex items-center gap-1.5 text-mute hover:text-fg transition-colors"
            >
              View as Markdown
              <ArrowUpRight className="size-3" />
            </a>
          </div>
          <div className="prose mt-8">
            <MDX components={getMDXComponents()} />
          </div>
        </div>
      </main>
      <Toc
        items={page.data.toc.map((item: TocEntry) => ({
          title: item.title,
          url: item.url,
          depth: item.depth,
        }))}
      />
    </>
  );
}

export async function generateStaticParams() {
  return source.generateParams();
}

export async function generateMetadata(props: PageProps<'/docs/[[...slug]]'>): Promise<Metadata> {
  const params = await props.params;
  const page = source.getPage(params.slug);
  if (!page) notFound();

  const suffix = page.slugs.length > 1 ? SECTION_TITLE_SUFFIX[page.slugs[0]] : undefined;
  const title = suffix ? `${page.data.title} — ${suffix}` : page.data.title;

  return {
    title,
    description: page.data.description,
    alternates: {
      canonical: page.url,
    },
    openGraph: {
      type: 'article',
      title,
      description: page.data.description,
      url: page.url,
      images: getPageImage(page).url,
    },
    twitter: {
      card: 'summary_large_image',
      title,
      description: page.data.description,
      images: getPageImage(page).url,
    },
  };
}
