import { codeFiles, getCodeFile, readCodeFile } from '@/lib/code';

// Plain-text mirror of the /code viewer, one URL per source file — what
// agents fetch. Prerendered at build; unknown paths 404.
export const dynamic = 'force-static';
export const dynamicParams = false;

export function generateStaticParams() {
  return codeFiles().map((file) => ({ slug: file.path.split('/') }));
}

export async function GET(
  _request: Request,
  { params }: { params: Promise<{ slug: string[] }> },
) {
  const { slug } = await params;
  const filePath = slug.join('/');
  if (!getCodeFile(filePath)) {
    return new Response('Not found', { status: 404 });
  }
  return new Response(readCodeFile(filePath), {
    headers: { 'Content-Type': 'text/plain; charset=utf-8' },
  });
}
