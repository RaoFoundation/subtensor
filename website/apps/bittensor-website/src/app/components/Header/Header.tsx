'use client';

import {useHamburgerMenuStore} from '@/app/stores/useHamburgerMenuStore';
import {Header as BaseHeader, Link as MenuLink} from '@raofoundation/ui';
import {AnimatePresence, motion} from 'framer-motion';
import React from 'react';
import ChevronIndicator from '../../../../public/svg/chevron_icon.svg';
import {HamburgerMenuItems} from '../HamBurgerMenuItems/HamburgerMenuItems';
import styles from './Header.module.css';
import {MenuSchema} from './MenuSchema';

type MenuItem = {
  label: string;
  href: string;
  isExternal: boolean;
};

export const Header = () => {
  const hamburgerMenuStore = useHamburgerMenuStore();

  return (
    <div className={styles.header_wrapper}>
      <BaseHeader
        hamberMenuItems={<HamburgerMenuItems />}
        hamburgerMenuIsOpened={hamburgerMenuStore.isVisible}
        hamburgerMenuToggle={hamburgerMenuStore.toggle}
        menu={<MenuComponent />}
      />
    </div>
  );
};

export const MenuComponent = () => {
  const about = MenuSchema.articles.find((item) => item.label.toLowerCase() === 'about');
  const docs = MenuSchema.docs[0];
  const whitepaper: MenuItem = {label: 'Whitepaper', href: '/whitepaper', isExternal: false};
  const wallet: MenuItem = {label: 'Wallet', href: '/wallet', isExternal: false};
  const explorers: MenuItem = {label: 'Explore', href: '/explore', isExternal: false};
  const discord = MenuSchema.connect.find((item) => item.label.toLowerCase() === 'discord');
  const menuItems = [about, whitepaper, docs, discord, wallet, explorers].filter(
    (item): item is MenuItem => Boolean(item)
  );

  return (
    <>
      {menuItems.map((item) => (
        <li className={styles.menu_item} key={item.href}>
          {MenuElement(item, false)}
        </li>
      ))}
    </>
  );
};

/**
 * Legacy dropdown menu retained for future use.
 * Preserve this component if the multi-section menu needs to be reinstated.
 */
export const LegacyMenuComponent = () => {
  const [articleDropdown, setArticleDropdown] = React.useState(false);
  const [researchDropdown, setResearchDropdown] = React.useState(false);
  const [connectDropdown, setConnectDropdown] = React.useState(false);

  return (
    <>
      <li className={styles.menu_item}>
        <motion.div
          className={styles.dropdown_menu_wrapper}
          onHoverStart={() => setArticleDropdown(true)}
          onHoverEnd={() => setArticleDropdown(false)}
        >
          <div style={{display: 'flex', gap: '1px', marginRight: '-10px'}}>
            <p className={styles.dropdown_item_label}>Explore</p> <ChevronIndicator />
          </div>
          <AnimatePresence>
            {articleDropdown && (
              <motion.div
                initial={{opacity: 0}}
                animate={{opacity: 1}}
                exit={{opacity: 0}}
                transition={{duration: 0.1, delay: 0.1}}
                className={styles.dropdown_articles_container}
              >
                {MenuSchema.articles.map((article) => (
                  <MenuLink href={article.href} isExternal={false} key={article.href}>
                    <p className={styles.dropdown_item_label}>{article.label}</p>
                  </MenuLink>
                ))}
              </motion.div>
            )}
          </AnimatePresence>
        </motion.div>
      </li>
      <li className={styles.menu_item}>{MenuElement(MenuSchema.docs[0], false)}</li>
      <li className={styles.menu_item}>
        <motion.div
          className={styles.dropdown_menu_wrapper}
          onHoverStart={() => setResearchDropdown(true)}
          onHoverEnd={() => setResearchDropdown(false)}
        >
          <div style={{display: 'flex', gap: '1px', marginRight: '-10px'}}>
            <p className={styles.dropdown_item_label}>Research</p> <ChevronIndicator />
          </div>
          <AnimatePresence>
            {researchDropdown && (
              <motion.div
                initial={{opacity: 0}}
                animate={{opacity: 1}}
                exit={{opacity: 0}}
                transition={{duration: 0.1, delay: 0.1}}
                className={styles.dropdown_articles_container}
              >
                {MenuSchema.research.map((research) => MenuElement(research, true))}
              </motion.div>
            )}
          </AnimatePresence>
        </motion.div>
      </li>
      <li className={styles.menu_item}>
        <motion.div
          className={styles.dropdown_menu_wrapper}
          onHoverStart={() => setConnectDropdown(true)}
          onHoverEnd={() => setConnectDropdown(false)}
        >
          <div style={{display: 'flex', gap: '1px', marginRight: '-10px'}}>
            <p className={styles.dropdown_item_label}>CONNECT</p> <ChevronIndicator />
          </div>
          <AnimatePresence>
            {connectDropdown && (
              <motion.div
                initial={{opacity: 0}}
                animate={{opacity: 1}}
                exit={{opacity: 0}}
                transition={{duration: 0.1, delay: 0.1}}
                className={styles.dropdown_articles_container}
              >
                {MenuSchema.connect.map((connect) => MenuElement(connect, true))}
              </motion.div>
            )}
          </AnimatePresence>
        </motion.div>
      </li>
    </>
  );
};

const MenuElement = (menuOption: (typeof MenuSchema)['articles'][0], isDropdown: boolean) => (
  <>
    <MenuLink href={menuOption.href} isExternal={menuOption.isExternal} key={menuOption.href}>
      <p className={isDropdown ? styles.dropdown_item_label : ''}>
        {menuOption.label.toUpperCase()}
      </p>
    </MenuLink>
  </>
);
