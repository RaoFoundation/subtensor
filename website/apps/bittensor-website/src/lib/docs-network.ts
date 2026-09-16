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
    installCommand: 'pip install "bittensor==11.3.0rc47"',
    installHint:
      'Install the pinned SDK release candidate from PyPI (11.3.0rc46 shipped a broken btcli; rc47 fixes it).',
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
