'use client';

import React, {FC, useState} from 'react';
import {Icon} from '../../Icon/Icon';

import styles from './CopyButton.module.css';
type CopyButtonProps = {
  copyText?: string;
  handleOnClick?: () => void;
};

export const CopyButton: FC<CopyButtonProps> = ({copyText, handleOnClick}) => {
  const [copyState, setCopyState] = useState('COPY');
  const handlelClick = async () => {
    //function to copy text to clipboard
    navigator.clipboard.writeText(copyText ? copyText : '');
    setCopyState('COPIED');
    await new Promise((r) => setTimeout(r, 500)).then(() => {
      handleOnClick ? handleOnClick() : null;
    });
  };
  return (
    <button className={styles.container} onClick={handlelClick}>
      <div>{copyState}</div>
      <div>
        <Icon.Copy />
      </div>
    </button>
  );
};
