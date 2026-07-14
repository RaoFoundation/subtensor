import type {Metadata} from 'next';
import WalletPage from './WalletPage';

export const metadata: Metadata = {
  title: 'TAO Wallets',
  description:
    'Wallets for holding TAO and interacting with the Bittensor network: browser extensions, ' +
    'mobile apps, and cold-storage options.',
  alternates: {canonical: '/wallet'},
};

export default function Page() {
  return <WalletPage />;
}
