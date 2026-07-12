import {FC} from 'react';
import {Icon} from '../Icon/Icon';
import Link from 'next/link';
import styles from './Navigation.module.css';

import {TaoCredit} from '../Display/TaoCredit/TaoCredit';
import {HamburgerMenu} from '../HamburgerMenu/HamburgerMenu';

export type NavigationProps = {
  links: Array<{url: string; label: string}>;
  taocredit?: number;
  logo?: JSX.Element;
  hideHamburger?: boolean;
};

export const Navigation: FC<NavigationProps> = ({links, taocredit, hideHamburger}) => {
  return (
    <nav className={styles.navbar_container}>
      <div>
        <Link href='/'>
          <Icon.TaoLogo />
        </Link>
      </div>
      <div className={styles.links_container}>
        {links.map((link, index) => (
          <a href={link.url} key={index} className={styles.links}>
            {link.label.toUpperCase()}
          </a>
        ))}
        {taocredit && (
          <div className={styles.tao_credit_container}>
            <TaoCredit taocredit={taocredit} />
          </div>
        )}
      </div>
      {!hideHamburger && (
        <div className={styles.hamburger_container}>
          <HamburgerMenu taocredit={taocredit} />
        </div>
      )}
    </nav>
  );
};
