import { source } from '@/lib/source';
import { llms } from 'fumadocs-core/source';
import type { Folder, Item, Node, Root } from 'fumadocs-core/page-tree';
import { buildCommit, CODE_ROOTS } from '@/lib/code';
import { chainRepoUrl, codeRoute, docsContentRoute, siteUrl } from '@/lib/shared';

export const revalidate = false;

/** Deep generated reference trees: landing page only in llms.txt. */
const COLLAPSE_PREFIXES = [
  '/docs/tx',
  '/docs/query',
  '/docs/errors',
  '/docs/hyperparameters',
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
        // Prefer real `index` when present; otherwise keep landing as sole child.
        if (node.index) return { ...node, children: [] };
        return { ...node, children: [landing] };
      }
      return { ...node, children: node.children.map(mapNode) };
    }
    return node;
  }

  return { ...root, children: root.children.map(mapNode) };
}

function searchSection(): string {
  return [
    '## Searching these docs',
    '',
    'Everything below is plain text over HTTP, so you can grep it as if it were local:',
    '',
    `- Curated index (this file): \`${siteUrl}/llms.txt\``,
    `- Full docs corpus (every page in one file, for \`rg\`, not for stuffing into context): \`curl -s ${siteUrl}/llms-full.txt | rg -n '<pattern>'\``,
    `- One page as markdown: ${siteUrl}${docsContentRoute}/<slug>/content.md, e.g. ${siteUrl}${docsContentRoute}/quickstart/content.md`,
    `- JSON catalogs: ${siteUrl}/catalog/intents.json, ${siteUrl}/catalog/reads.json, ${siteUrl}/catalog/errors.json`,
    `- Chain source: \`curl -s ${siteUrl}${codeRoute}/index.json\` lists every file path, then \`curl -s ${siteUrl}${codeRoute}/raw/<repo-path> | rg -n '<pattern>'\``,
    `- For heavy exploration, clone the repo (docs live in docs/): \`git clone --depth 1 ${chainRepoUrl}\``,
    `- Human-readable guide: ${siteUrl}/docs/agents#searching-these-docs`,
    '',
  ].join('\n');
}

function referenceHint(): string {
  return [
    '## Reference catalogs',
    '',
    'Per-op and per-query pages are omitted from this index (they dominate the tree).',
    'Use the landing pages and JSON catalogs instead:',
    '',
    `- [Transactions](${siteUrl}/docs/tx) / ${siteUrl}/catalog/intents.json`,
    `- [Queries](${siteUrl}/docs/query) / ${siteUrl}/catalog/reads.json`,
    `- [Errors](${siteUrl}/docs/errors) / ${siteUrl}/catalog/errors.json`,
    `- [Hyperparameters](${siteUrl}/docs/hyperparameters)`,
    `- Full prose dump of every page: ${siteUrl}/llms-full.txt`,
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
    `- [Index of every file (JSON)](${codeRoute}/index.json)`,
    `- Browse: ${codeRoute}/<repo-path> with #L<n> or #L<n>-L<m> line anchors, e.g. ${codeRoute}/pallets/subtensor/src/coinbase/run_coinbase.rs`,
    `- Plain text: ${codeRoute}/raw/<repo-path>`,
    `- Roots: ${CODE_ROOTS.join(', ')}`,
    '',
  ].join('\n');
}

export function GET() {
  // H1 first (llms.txt convention), then search tips before the page list.
  const slimmed = slimTree(source.getPageTree());
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
