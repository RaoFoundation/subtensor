export type DocsNetwork = 'devnet' | 'testnet';

export type DocsNetworkInfo = {
  network: DocsNetwork;
  label: string;
  /** SDK / btcli network flag for this host. */
  chainNetwork: string;
  installCommand: string;
  installHint: string;
};

const NETWORKS: Record<DocsNetwork, DocsNetworkInfo> = {
  devnet: {
    network: 'devnet',
    label: 'devnet',
    chainNetwork: 'devnet',
    installCommand:
      'pip install --index-url https://test.pypi.org/simple/ --extra-index-url https://pypi.org/simple/ --pre bittensor',
    installHint: 'Install the matching SDK from TestPyPI (pre-release .dev builds).',
  },
  testnet: {
    network: 'testnet',
    label: 'testnet',
    chainNetwork: 'test',
    installCommand: 'pip install --pre bittensor',
    installHint: 'Install the matching SDK rc from PyPI with --pre.',
  },
};

/** Build-time channel set by deploy-docs.yml for network-mirror hosts. */
export function getDocsNetwork(): DocsNetworkInfo | null {
  const raw = process.env.NEXT_PUBLIC_DOCS_NETWORK;
  if (raw === 'devnet' || raw === 'testnet') {
    return NETWORKS[raw];
  }
  return null;
}
