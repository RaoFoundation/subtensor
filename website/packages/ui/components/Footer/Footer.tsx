'use client';

import React, {FC, useState} from 'react';
import styles from './Footer.module.css';
import {Icon} from '../Icon/Icon';
import {DropDownMenu} from '../DropDownMenu/DropDownMenu';
import clsx from 'clsx';

export type FooterProps = {
  newChat?: () => void;
  showMenu?: boolean;
};

export const Footer: FC<FooterProps> = ({newChat, showMenu}) => {
  const [displayModal, setDisplayModal] = useState(false);

  const handleOnClick = () => {
    setDisplayModal(!displayModal);
  };

  return (
    <>
      <DropDownMenu isVisible={displayModal} newChat={newChat} close={handleOnClick} />
      <div className={styles.footer}>
        <span
          onClick={handleOnClick}
          className={clsx(styles.menu_container, !showMenu && styles.hidden)}
        >
          <span className={styles.icon}>
            <Icon.Menu />
          </span>
          <span className={clsx(styles.icon, displayModal && styles.reversed)}>
            <Icon.DownArrow />
          </span>
        </span>
      </div>
    </>
  );
};
