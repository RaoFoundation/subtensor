'use client';

import React, {useState} from 'react';
import styles from './CodeSection.module.css';
import {Icon} from '@raofoundation/ui';
import clsx from 'clsx';

export const CodeSection = () => {
  const [copyState, setCopyState] = useState(false);

  const copyCode = async () => {
    navigator.clipboard.writeText(
      '/bin/bash -c “$(curl -fsSL https://raw.githubusercontent.com/opentensor/bittensor/master/scripts/install.sh)“',
    );
    setCopyState(true);
    await new Promise((r) => setTimeout(r, 500)).then(() => {
      setCopyState(false);
    });
  };
  return (
    <section className={styles.section_code}>
      <div className={styles.section_code_text}>
        <p>STEP 01/ INSTALL</p>
        <p>pip install bittensor</p>
      </div>
      <div className={styles.section_code_text}>
        <p>STEP 02/ ACCESS</p>
        <p>{`bt.prompt('hello world')`}</p>
      </div>
      <div className={styles.section_code_text}>
        <p>Step 03/ MINE</p>
        <div className={styles.copy_code} onClick={copyCode}>
          <p className={styles.section_code_truncated}>
            /bin/bash -c &ldquo;$(curl -fsSL
            https://raw.githubusercontent.com/opentensor/bittensor/master/scripts/install.sh)&ldquo;
          </p>
          <div className={styles.copy_code_icon}>
            <span
              className={clsx(styles.copy_code_copied_text, copyState && styles.copy_code_active)}
            >
              <Icon.Copy />
              {copyState && ' Copied!'}
            </span>
          </div>
        </div>
      </div>
    </section>
  );
};
