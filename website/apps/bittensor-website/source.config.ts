import { defineConfig, defineDocs, frontmatterSchema, metaSchema } from 'fumadocs-mdx/config';
// The unified-11 releases, aliased so the unified-10 versions can stay pinned
// for the legacy react-markdown pages.
import rehypeKatex from 'rehype-katex-v7';
import remarkMath from 'remark-math-v6';
import { btDark, btLight } from './src/lib/shiki-themes';

// The schemas must come from fumadocs-mdx (bundled zod 4 types), not
// fumadocs-core/source/schema: fumadocs-core's declarations resolve this
// app's zod 3 (kept for tRPC), which breaks the collection typing.
export const docs = defineDocs({
  // Docs are the repo-wide source of truth and live at the repository root
  // (subtensor/docs), outside this app. The deploy workflow builds from a full
  // checkout, so the relative path is always resolvable.
  dir: '../../../docs',
  docs: {
    schema: frontmatterSchema,
    postprocess: {
      includeProcessedMarkdown: true,
    },
  },
  meta: {
    schema: metaSchema,
  },
});

export default defineConfig({
  mdxOptions: {
    remarkPlugins: [remarkMath],
    // Prepend: KaTeX must transform math nodes before fumadocs' rehype-code
    // tries to syntax-highlight them as `language-math` blocks.
    rehypePlugins: (v) => [rehypeKatex, ...v],
    rehypeCodeOptions: {
      themes: {
        light: btLight,
        dark: btDark,
      },
    },
  },
});
