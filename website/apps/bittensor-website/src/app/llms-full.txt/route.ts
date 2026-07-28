import { getLLMText, source } from '@/lib/source';
import { codeRoute } from '@/lib/shared';

export const revalidate = false;

export async function GET() {
  const scan = source.getPages().map(getLLMText);
  const scanned = await Promise.all(scan);

  const codePointer = `Chain Rust source is browsable at ${codeRoute}/<repo-path> (index: ${codeRoute}/index.json).`;

  return new Response(`${scanned.join('\n\n')}\n\n${codePointer}`);
}
