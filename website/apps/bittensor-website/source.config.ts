import { defineConfig, defineDocs, frontmatterSchema, metaSchema } from 'fumadocs-mdx/config';
// The unified-11 releases, aliased so the unified-10 versions can stay pinned
// for the legacy react-markdown pages.
import rehypeKatex from 'rehype-katex-v7';
import remarkMath from 'remark-math-v6';

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

// Two-tone monochrome code themes: ink for code, mute for the parts you skim
// past (comments, strings, punctuation) — the last color on the site removed.
function monochromeTheme(name: string, type: 'light' | 'dark', ink: string, mute: string) {
  return {
    name,
    type,
    colors: {
      'editor.background': 'transparent',
      'editor.foreground': ink,
    },
    tokenColors: [
      { settings: { foreground: ink } },
      {
        scope: ['comment', 'punctuation.definition.comment'],
        settings: { foreground: mute, fontStyle: 'italic' },
      },
      {
        scope: ['string', 'string.quoted', 'constant.numeric', 'constant.language'],
        settings: { foreground: mute },
      },
      {
        scope: ['keyword', 'storage.type', 'storage.modifier', 'keyword.control'],
        settings: { foreground: ink, fontStyle: 'bold' },
      },
      {
        scope: ['punctuation', 'meta.brace'],
        settings: { foreground: mute },
      },
    ],
  };
}

export default defineConfig({
  mdxOptions: {
    remarkPlugins: [remarkMath],
    // Prepend: KaTeX must transform math nodes before fumadocs' rehype-code
    // tries to syntax-highlight them as `language-math` blocks.
    rehypePlugins: (v) => [rehypeKatex, ...v],
    rehypeCodeOptions: {
      themes: {
        light: monochromeTheme('bt-light', 'light', '#292929', '#8a8a8a'),
        dark: monochromeTheme('bt-dark', 'dark', '#ebebeb', '#7d7d7d'),
      },
    },
  },
});
