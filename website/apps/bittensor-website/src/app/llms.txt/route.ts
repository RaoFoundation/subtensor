import { source } from '@/lib/source';
import { llms } from 'fumadocs-core/source';
import type { Folder, Item, Node, Root } from 'fumadocs-core/page-tree';
import { buildCommit, CODE_ROOTS } from '@/lib/code';
import { chainRepoUrl, codeRoute, docsContentRoute, docsRoute, siteUrl } from '@/lib/shared';

export const revalidate = false;

/** Deep generated reference trees: landing page only in llms.txt. */
const COLLAPSE_PREFIXES = [
  `${docsRoute}/tx`,
  `${docsRoute}/query`,
  `${docsRoute}/errors`,
  `${docsRoute}/hyperparameters`,
];

function isCollapseLanding(url: string): boolean {
  return COLLAPSE_PREFIXES.some((prefix) => url === prefix);
}

/** Fumadocs often puts the folder landing page in `children[0]`, not `index`. */
function landingPage(node: Folder): Item | undefined {
  if (node.index) return node.index;
  return node.children.find((child): child is Item => child.type === 'page');
}

function shouldCollapse(node: Folder): boolean {
  const landing = landingPage(node);
  return landing !== undefined && isCollapseLanding(landing.url);
}

/** Drop children of deep reference folders; keep the landing page. */
function slimTree(root: Root): Root {
  function mapNode(node: Node): Node {
    if (node.type === 'folder') {
      if (shouldCollapse(node)) {
        const landing = landingPage(node);
        if (!landing) return { ...node, children: [] };
        if (node.index) return { ...node, children: [] };
        return { ...node, children: [landing] };
      }
      return { ...node, children: node.children.map(mapNode) };
    }
    return node;
  }

  return { ...root, children: root.children.map(mapNode) };
}

/**
 * Point index links at fetchable markdown (or absolute site URLs), so an agent
 * never has to rewrite `/docs/foo` → `/llms.mdx/docs/foo/content.md`.
 */
function toAgentUrl(url: string): string {
  if (url === docsRoute || url === `${docsRoute}/`) {
    return `${siteUrl}${docsContentRoute}/content.md`;
  }
  if (url.startsWith(`${docsRoute}/`)) {
    const slug = url.slice(docsRoute.length + 1).replace(/\/$/, '');
    return `${siteUrl}${docsContentRoute}/${slug}/content.md`;
  }
  if (url.startsWith('/')) return `${siteUrl}${url}`;
  return url;
}

function rewriteUrls(root: Root): Root {
  function mapNode(node: Node): Node {
    if (node.type === 'page') {
      return { ...node, url: toAgentUrl(node.url) };
    }
    if (node.type === 'folder') {
      return {
        ...node,
        index: node.index ? { ...node.index, url: toAgentUrl(node.index.url) } : undefined,
        children: node.children.map(mapNode),
      };
    }
    return node;
  }

  return { ...root, children: root.children.map(mapNode) };
}

function searchSection(): string {
  return [
    '## Searching these docs',
    '',
    'Every docs link in this file is already raw markdown. Fetch it directly;',
    'do not scrape HTML. Grep the corpus as if it were local:',
    '',
    `- This index: \`${siteUrl}/llms.txt\``,
    `- Full corpus (for \`rg\`, not for stuffing into context): \`curl -s ${siteUrl}/llms-full.txt | rg -n '<pattern>'\``,
    `- One page: ${siteUrl}${docsContentRoute}/<slug>/content.md`,
    `- JSON catalogs: ${siteUrl}/catalog/intents.json, ${siteUrl}/catalog/reads.json, ${siteUrl}/catalog/errors.json`,
    `- Chain source: \`curl -s ${siteUrl}${codeRoute}/index.json\` then \`curl -s ${siteUrl}${codeRoute}/raw/<repo-path>\``,
    `- Clone for heavy exploration (docs in docs/): \`git clone --depth 1 ${chainRepoUrl}\``,
    '',
  ].join('\n');
}

function referenceHint(): string {
  return [
    '## Reference catalogs',
    '',
    'Per-op and per-query pages are omitted here (they dominate the tree).',
    'Use the JSON catalogs; open a docs_url from an entry only when you need prose.',
    `Catalog \`docs_url\` values look like \`/docs/tx/add-stake\`; the markdown is`,
    `${siteUrl}${docsContentRoute}/tx/add-stake/content.md` +
      ' (prefix `/docs/` → `/llms.mdx/docs/`, suffix `/content.md`).',
    '',
    `- Transactions: ${siteUrl}/catalog/intents.json`,
    `- Queries: ${siteUrl}/catalog/reads.json`,
    `- Errors: ${siteUrl}/catalog/errors.json` +
      ' (semantic codes under `.codes`, chain names under `.chain_errors`)',
    `- Hyperparameters landing: ${siteUrl}${docsContentRoute}/hyperparameters/content.md`,
    `- Full prose dump: ${siteUrl}/llms-full.txt`,
    '',
  ].join('\n');
}

function codeSection(): string {
  const commit = buildCommit();
  return [
    '## Chain source code',
    '',
    'The Rust that runs on the chain (pallets, runtime, primitives — no tests/mocks/benchmarks)' +
      (commit ? `, at commit ${commit}.` : '.'),
    '',
    `- Index: ${siteUrl}${codeRoute}/index.json`,
    `- Browse: ${siteUrl}${codeRoute}/<repo-path> with #L<n> or #L<n>-L<m> line anchors`,
    `- Plain text: ${siteUrl}${codeRoute}/raw/<repo-path>`,
    `- Roots: ${CODE_ROOTS.join(', ')}`,
    '',
  ].join('\n');
}

export function GET() {
  const slimmed = rewriteUrls(slimTree(source.getPageTree()));
  const formatter = llms(source);
  const title = typeof slimmed.name === 'string' ? slimmed.name : 'Bittensor';
  const description =
    typeof slimmed.description === 'string' ? `> ${slimmed.description}\n\n` : '';
  const tree = slimmed.children.map((child) => formatter.indexNode(child)).join('\n');

  return new Response(
    [
      `# ${title}`,
      '',
      description + searchSection(),
      referenceHint(),
      tree,
      '',
      codeSection(),
    ].join('\n'),
  );
}
