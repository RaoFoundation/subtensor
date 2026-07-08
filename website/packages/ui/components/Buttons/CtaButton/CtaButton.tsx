import clsx from 'clsx';
import React, {MouseEventHandler} from 'react';
import {FC} from 'react';
import styles from './CtaButton.module.css';

export type ButtonProps = {
  label: string;
  onClick?: MouseEventHandler<HTMLButtonElement>;
  isBusy?: boolean;
  isDisabled?: boolean;
};

export const CtaButton: FC<ButtonProps> = ({label, onClick, isBusy, isDisabled}) => {
  return (
    <div className={styles.container}>
      <button
        className={clsx(styles.button, {[styles.busy]: isBusy, [styles.disabled]: isDisabled})}
        onClick={onClick}
      >
        {label}
      </button>
    </div>
  );
};
