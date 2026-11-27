import type {Metadata} from 'next';
import SwapPage from './SwapPage';

export const metadata: Metadata = {
  title: 'Swap',
  description:
    'Buy canonical TAO, subnet alpha tokens and service credits with USDC from an Ethereum ' +
    'wallet. Every token is backed on Bittensor.',
  alternates: {canonical: '/swap'},
};

export default function Page() {
  return <SwapPage />;
}
