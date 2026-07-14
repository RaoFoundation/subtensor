'use client';

import React, {FC} from 'react';
import clsx from 'clsx';
import styles from './BaseIconButton.module.css';

import {Icon} from '../../Icon/Icon';

interface BaseIconButtonProps {
  label: string;
  isLogoUp?: boolean;
  children: JSX.Element;
  handleOnClick?: () => void;
}

export const BaseIconButton: FC<BaseIconButtonProps> = ({
  label,
  children,
  isLogoUp,
  handleOnClick,
}) => {
  return (
    <button
      className={clsx(styles.container, isLogoUp ? styles.logo_up : styles.logo_down)}
      onClick={handleOnClick}
    >
      <div>{label}</div>
      <div className={styles.icon_wrapper}>{children}</div>
    </button>
  );
};

//styles.container
