import React, {FC} from 'react';
import styles from './Switch.module.css';

const sliderClasses = styles.switch + ' ' + styles.input;

export const Switch: FC = () => {
  return (
    <div>
      <label className={sliderClasses}>
        <input type='checkbox' />
        <span className={styles.slider}></span>
      </label>
    </div>
  );
};
