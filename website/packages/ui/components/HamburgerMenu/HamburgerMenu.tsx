'use client';

import React, {FC, useEffect, useState} from 'react';
import styles from './HamburgerMenu.module.css';

import {Icon} from '../Icon/Icon';

export type HamburgerMenuProps = {
  taocredit?: number;
  children?: React.ReactNode;
  // If no toggle & isOpened is provided, the menu
  // will be controlled by the component's state.
  toggle?: () => void;
  isOpened?: boolean;
};

export const HamburgerMenu: FC<HamburgerMenuProps> = ({children, toggle, isOpened}) => {
  const [displayMenu, setDisplayMenu] = useState(!!isOpened);

  const handleOnClick = () => {
    toggle ? toggle() : setDisplayMenu(!displayMenu);
  };

  useEffect(() => {
    setDisplayMenu(!!isOpened);
  }, [isOpened]);

  if (displayMenu) {
    return (
      <div className={styles.hamburger_menu}>
        <button className={styles.close_btn} onClick={handleOnClick}>
          <Icon.Close />
        </button>
        {children}
      </div>
    );
  } else {
    return (
      <button className={styles.hamburger_btn} onClick={handleOnClick}>
        <Icon.Hamburger />
      </button>
    );
  }
};
