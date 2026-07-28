export const MenuSchema = {
  articles: [
    {label: 'About', href: '/about', isExternal: false},
    {label: 'Intro', href: '/intro', isExternal: false},
    {label: 'Philosophy', href: '/content/the-bittensor-standard', isExternal: false},
  ],
  research: [
    {label: 'Whitepaper', href: '/whitepaper', isExternal: false},
    {label: 'Consensus V2', href: '/content/consensus_v2', isExternal: false},
    {
      label: 'DTAO Whitepaper',
      href: '/dtao-whitepaper',
      isExternal: false,
    },
  ],
  connect: [
    {label: 'X', href: 'https://x.com/bittensor', isExternal: true},
    {label: 'DISCORD', href: 'https://discord.gg/qasY3HA9F9', isExternal: true},
    {label: 'GITHUB', href: 'https://github.com/RaoFoundation', isExternal: true},
  ],
  docs: [{label: 'DOCS', href: '/docs', isExternal: false}],
};
