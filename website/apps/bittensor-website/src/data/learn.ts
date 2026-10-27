import type {ExplorerCardData} from './explorers';

export const learnResources: ExplorerCardData[] = [
  // Our own documentation. Keep this entry first so it is always the
  // highest-priority resource in the Learn section.
  {
    id: 'bittensordocs',
    name: 'Bittensor Docs',
    href: '/docs',
    badge: 'Official',
    imageSrc: '/assets/learn/bittensor-docs-card.png',
    leftIconSrc: '/assets/learn/bittensor-docs-icon.svg',
    logoSrc: '/assets/learn/bittensor-docs-logo.svg',
  },
  {
    id: 'chainsource',
    name: 'Chain source',
    href: '/code',
    badge: 'Official',
    imageSrc: '/assets/learn/chain-source-card.svg',
    logoSrc: '/assets/learn/chain-source-logo.svg',
  },
  {
    id: 'taostatsdocs',
    name: 'Taostats Docs',
    href: 'https://docs.taostats.io',
    imageSrc: '/assets/learn/taostats-docs-card.png',
    leftIconSrc: '/assets/learn/taostats-docs-icon.svg',
    logoSrc: '/assets/learn/taostats-docs-logo.svg',
  },
  {
    id: 'learnbittensor',
    name: 'Learn Bittensor',
    href: 'https://learnbittensor.org',
    imageSrc: '/assets/learn/learnbittensor-card.png',
    leftIconSrc: '/assets/learn/learnbittensor-icon.svg',
    logoSrc: '/assets/learn/learnbittensor-logo.svg',
  },
];
