import React, {FC} from 'react';
import {Link} from '../Link/Link';
import styles from './BaseFooter.module.css';
import clsx from 'clsx';

export type LinkProps = {
  id: string;
  url: string;
  isExternal: boolean;
  title: string;
};

type ModuleProps = {
  id: number;
  title: string;
  links: LinkProps[];
};

type BaseFooterColumn = {
  title?: string;
  modules: ModuleProps[];
  className?: string;
};

export const BaseFooter: FC<BaseFooterColumn> = ({modules, className}) => {
  return (
    <footer className={clsx(styles.footer, className)}>
      {/* <p className={styles.footer_title}>{content.title}</p> */}

      <ul className={styles.footer_modules}>
        {modules.map((module) => (
          <li key={`footer-module-${module.id}`}>
            <span className={styles.footer_moduleTitle}>{module.title}</span>
            <ul className={styles.footer_links}>
              {module.links.map((link) => (
                <li key={`footer-module-${module.id}-link-${link.id}`}>
                  <Link href={link.url} isExternal={link.isExternal}>
                    {link.title}
                  </Link>
                </li>
              ))}
            </ul>
          </li>
        ))}
      </ul>
    </footer>
  );
};
