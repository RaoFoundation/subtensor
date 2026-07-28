'use client';

import React, {FC, ReactNode} from 'react';
import {TaoCredit} from '../Display/TaoCredit/TaoCredit';
import {HamburgerMenu} from '../HamburgerMenu/HamburgerMenu';
import {Icon} from '../Icon/Icon';

import NextLink from 'next/link';
import {usePathname} from 'next/navigation';
import styles from './Header.module.css';

export type HeaderProps = {
  taocredit?: number;
  walletAction?: () => void;
  logo?: JSX.Element;
  hideHamburger?: boolean;
  hamberMenuItems?: React.ReactNode;
  hamburgerMenuToggle?: () => void;
  hamburgerMenuIsOpened?: boolean;
  children?: JSX.Element;
  menu?: ReactNode;
};

export const Header: FC<HeaderProps> = ({
  taocredit,
  walletAction,
  logo,
  hideHamburger,
  hamberMenuItems,
  hamburgerMenuToggle,
  hamburgerMenuIsOpened,
  menu,
}) => {
  const hasTaoCredit = taocredit !== undefined;

  const pathname = usePathname();

  return (
    <div className={styles.container}>
      <div className={styles.tao_logo}>
        <NextLink href='/'>{logo ?? <Icon.TaoLogoLg />}</NextLink>
      </div>

      <ul className={styles.menu_list}>{menu && menu}</ul>

      {!hideHamburger && (
        <div className={styles.hamburger_container}>
          <HamburgerMenu
            taocredit={taocredit}
            toggle={hamburgerMenuToggle}
            isOpened={hamburgerMenuIsOpened}
          >
            {hamberMenuItems}
          </HamburgerMenu>
        </div>
      )}
      {hasTaoCredit && (
        <div className={styles.tao_credit_container}>
          <TaoCredit taocredit={taocredit} walletAction={walletAction} />
        </div>
      )}
      {!hasTaoCredit && <div className={styles.right_spacer} aria-hidden />}
    </div>
  );
};
