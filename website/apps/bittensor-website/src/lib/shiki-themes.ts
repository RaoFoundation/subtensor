// Two-tone monochrome code themes: ink for code, mute for the parts you skim
// past (comments, strings, punctuation) — the last color on the site removed.
// Shared by the MDX pipeline (source.config.ts) and the /code source viewer.
export function monochromeTheme(
  name: string,
  type: 'light' | 'dark',
  ink: string,
  mute: string,
) {
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

export const btLight = monochromeTheme('bt-light', 'light', '#292929', '#8a8a8a');
export const btDark = monochromeTheme('bt-dark', 'dark', '#ebebeb', '#7d7d7d');
