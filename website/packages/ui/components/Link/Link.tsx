import NextLink from 'next/link';
import clsx from 'clsx';

import {FC} from 'react';

import styles from './Link.module.css';

type LinkProps = {
  href: string;
  isExternal?: boolean;
  className?: string;
  children: React.ReactNode;
  isLinkButton?: boolean;
  [key: string]: any;
  onClick?: () => void;
};

export const Link: FC<LinkProps> = ({
  href,
  isExternal,
  className,
  children,
  isLinkButton,
  onClick,
  ...props
}) => {
  return (
    <NextLink
      href={href}
      className={clsx(styles.link, className, {'is-button': isLinkButton})}
      target={isExternal ? '_blank' : '_self'}
      scroll={false}
      onClick={onClick}
      {...props}
    >
      {children}
    </NextLink>
  );
};
