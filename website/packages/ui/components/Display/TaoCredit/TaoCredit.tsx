import React, {FC} from 'react';
import styles from './TaoCredit.module.css';
import {Typography} from '../../Typography/Typography';
import {Link} from '../../Link/Link';

export type TaoCreditProps = {
  taocredit: number;
  walletAction?: () => void;
};

export const TaoCredit: FC<TaoCreditProps> = ({taocredit, walletAction}) => {
  const balance = prettifyAmount(taocredit || 0);
  return (
    <div className={styles.tao_credit_val}>
      {/* Uncomment once  wallet will be ready */}
      {/* <Link href='' isLinkButton={true} onClick={walletAction}>
        <Typography.Cta2>{balance}</Typography.Cta2>
      </Link> */}
      <Link href='https://bittensor.com/wallet' isLinkButton={true}>
        <span>&tau;</span>
        <span>{balance}</span>
      </Link>
    </div>
  );
};

function prettifyAmount(amount: number) {
  return (amount / 1000000000).toFixed(2);
}
