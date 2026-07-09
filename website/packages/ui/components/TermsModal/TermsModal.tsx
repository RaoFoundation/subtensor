import React from 'react';
import styles from './TermsModal.module.css';

import {CtaButton} from '../Buttons/CtaButton/CtaButton';

export const TermsModal = () => {
  return (
    <div className={styles.container}>
      <div className={styles.header}>
        This is a non-custodial wallet created by the <br />
        Rao Foundation.
      </div>
      <div className={styles.terms}>
        <p>
          You can use this wallet to store and transfer TAO, even if you don’t have a miner
          <br />
          running in the network.
        </p>
        <p>
          We recommend you exercise caution when using this device: securely store all
          <br />
          mnemonics and passwords created and use the “forget” option to clear browser
          <br />
          information between uses - our browser will automatically store it.
        </p>
        <p>
          Lastly, always ensure that you are on the official bittensor.com while using this
          <br />
          wallet.
        </p>
      </div>
      <CtaButton label='CONFIRM AND ACCEPT' />
    </div>
  );
};
