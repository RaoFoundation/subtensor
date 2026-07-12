import React, {FC} from 'react';
import styles from './SubtensorInfo.module.css';
import {CountUp} from '../CountUp/CountUp';
import {Loader} from '@raofoundation/ui';

type SubtensorInfoProps = {
  isLoading: boolean;
  daysRunning: number;
  activeNodes: number;
  taoStaked: number;
};

export const SubtensorInfo: FC<SubtensorInfoProps> = ({
  isLoading,
  daysRunning,
  activeNodes,
  taoStaked,
}) => {
  return (
    <div className={styles.info}>
      <div className={styles.info_container}>
        {isLoading ? <Loader></Loader> : <CountUp value={daysRunning} />}
        <p className={styles.info_text}>Days running</p>
      </div>
      <div className={styles.info_container}>
        {isLoading ? <Loader></Loader> : <CountUp value={activeNodes} />}
        <p className={styles.info_text}>Active keys</p>
      </div>
      <div className={styles.info_container}>
        {isLoading ? <Loader></Loader> : <CountUp value={taoStaked} />}
        <p className={styles.info_text}>Tao staked</p>
      </div>
    </div>
  );
};
