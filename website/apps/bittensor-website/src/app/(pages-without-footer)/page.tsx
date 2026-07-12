import type {Metadata} from 'next';
import {Canvas} from '@/app/components/Canvas/Canvas';
import styles from './page.module.css';

export const metadata: Metadata = {
  title: {absolute: 'Bittensor — decentralized machine intelligence network'},
  description:
    'Bittensor is an open network where subnets produce digital commodities — ' +
    'compute, inference, storage, prediction — and contributors earn TAO. ' +
    'Build with the Bittensor Python SDK and btcli.',
  alternates: {
    canonical: '/',
  },
};

export default function Page() {
  return (
    <div className={styles.container}>
      <h1 className='sr_only'>Bittensor — decentralized machine intelligence network</h1>
      <p className='sr_only'>
        Bittensor is an open network where independent subnets produce digital commodities and the
        chain pays contributors in TAO. Explore the documentation for the Bittensor Python SDK and
        btcli command line: create a wallet, stake TAO, mine, validate, and run subnets.
      </p>
      <div className={styles.e8_canvas}>
        <Canvas />
      </div>
    </div>
  );
}
