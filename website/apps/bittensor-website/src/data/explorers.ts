export type ExplorerCardData = {
  id: string;
  name: string;
  href: string;
  imageSrc: string;
  logoSrc: string;
  badge?: string;
} & (
  | {leftIconSrc?: undefined; leftIconMaskSrc?: undefined}
  | {leftIconSrc: string; leftIconMaskSrc?: undefined}
  | {leftIconSrc: string; leftIconMaskSrc: string}
);

export const explorers: ExplorerCardData[] = [
  {
    id: 'taostats',
    name: 'Taostats',
    href: 'https://taostats.io',
    imageSrc: '/assets/explorers/taostats-card.png',
    leftIconSrc: '/assets/explorers/taostats-icon.svg',
    logoSrc: '/assets/explorers/taostats-logo.svg',
  },
  {
    id: 'taoapp',
    name: 'Tao App',
    href: 'https://www.tao.app/explorer',
    imageSrc: '/assets/explorers/taoapp-card.png',
    leftIconSrc: '/assets/explorers/taoapp-logo-icon.svg',
    logoSrc: '/assets/explorers/taoapp-logo-text.svg',
  },
  {
    id: 'backprop',
    name: 'Backprop',
    href: 'https://backprop.finance/screener',
    imageSrc: '/assets/explorers/backprop-card.png',
    leftIconSrc: '/assets/explorers/backprop-icon-art.svg',
    leftIconMaskSrc: '/assets/explorers/backprop-icon-mask.svg',
    logoSrc: '/assets/explorers/backprop-logo.svg',
  },
  {
    id: 'taomarketcap',
    name: 'Tao Market Cap',
    href: 'https://taomarketcap.com',
    imageSrc: '/assets/explorers/taomarketcap-card.png',
    logoSrc: '/assets/explorers/taomarketcap-logo.svg',
  },
  {
    id: 'taobot',
    name: 'TAO.bot',
    href: 'https://www.tao.bot/explore',
    imageSrc: '/assets/explorers/taobot-card.png',
    logoSrc: '/assets/explorers/taobot-logo.svg',
  },
  {
    id: 'taoswap',
    name: 'Taoswap',
    href: 'https://taoswap.org/explore/subnets',
    imageSrc: '/assets/explorers/taoswap-card.png',
    leftIconSrc: '/assets/explorers/taoswap-icon.svg',
    logoSrc: '/assets/explorers/taoswap-logo.svg',
  },
  {
    id: 'bittensorai',
    name: 'Bittensor.ai',
    href: 'https://bittensor.ai/subnets',
    imageSrc: '/assets/explorers/bittensorai-card.png',
    logoSrc: '/assets/explorers/bittensorai-logo.svg',
  },
  {
    id: 'subnetai',
    name: 'Subnet.ai',
    href: 'https://subnet.ai',
    imageSrc: '/assets/explorers/subnetai-card.png',
    leftIconSrc: '/assets/explorers/subnetai-icon.svg',
    logoSrc: '/assets/explorers/subnetai-logo.svg',
  },
  {
    id: 'taoflows',
    name: 'TaoFlows',
    href: 'https://taoflows.app',
    imageSrc: '/assets/explorers/taoflows-card.png',
    logoSrc: '/assets/explorers/taoflows-logo.svg',
  },
  {
    id: 'subnetradar',
    name: 'Subnet Radar',
    href: 'https://subnetradar.com',
    imageSrc: '/assets/explorers/subnetradar-card.png',
    leftIconSrc: '/assets/explorers/subnetradar-icon.svg',
    logoSrc: '/assets/explorers/subnetradar-logo.svg',
  },
  {
    id: 'taoxyz',
    name: 'TAO.xyz',
    href: 'https://tao.xyz',
    imageSrc: '/assets/explorers/taoxyz-card.png',
    leftIconSrc: '/assets/explorers/taoxyz-icon.svg',
    logoSrc: '/assets/explorers/taoxyz-logo.svg',
  },
];
