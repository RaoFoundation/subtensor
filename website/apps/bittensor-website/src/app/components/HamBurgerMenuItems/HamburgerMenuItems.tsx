import {useHamburgerMenuStore} from '@/app/stores/useHamburgerMenuStore';
import {Link} from '@raofoundation/ui';
import {FC} from 'react';
import {Footer} from '../Footer/Footer';
import styles from './HamburgerMenuItems.module.css';
import {MenuSchema} from '../Header/MenuSchema';

type MenuItem = {
  label: string;
  href: string;
  isExternal: boolean;
};

/**
 * Legacy menu generator retained for quick reactivation of the multi-section menu.
 */
export const buildLegacyMenuData = () => [
  ...MenuSchema.articles,
  ...MenuSchema.research,
  ...MenuSchema.docs,
];

const wallet: MenuItem = {label: 'Wallet', href: '/wallet', isExternal: false};
const explorers: MenuItem = {label: 'Explore', href: '/explore', isExternal: false};
const discord = MenuSchema.connect.find((item) => item.label.toLowerCase() === 'discord');
const whitepaper: MenuItem = {label: 'Whitepaper', href: '/whitepaper', isExternal: false};

const menuData: MenuItem[] = [
  MenuSchema.articles.find((item) => item.label.toLowerCase() === 'about'),
  whitepaper,
  MenuSchema.docs[0],
  discord,
  wallet,
  explorers,
].filter((item): item is MenuItem => Boolean(item));

export const HamburgerMenuItems: FC = () => {
  const hamburgerMenuStore = useHamburgerMenuStore();
  return (
    <div className={styles.menu_wrapper}>
      <div className={styles.link_container}>
        {menuData.map((item) => (
          <Link
            key={item.href}
            href={item.href}
            isExternal={item.isExternal}
            onClick={hamburgerMenuStore.toggle}
          >
            <p className={styles.menu_link}>{item.label}</p>
          </Link>
        ))}
      </div>
      <div className={styles.footer_wrapper}>
        <Footer isHamburger />
      </div>
    </div>
  );
};
