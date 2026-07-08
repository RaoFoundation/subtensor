import clsx from 'clsx';
import {FC} from 'react';

import styles from './Typography.module.css';

export type TypographyProps = {
  className?: string;
  children?: React.ReactNode;
};

export const Typography = {
  Text: ({className, children}: TypographyProps) => (
    <span className={clsx(styles.text, className)}>{children}</span>
  ),
  H1: ({className, children}: TypographyProps) => (
    <span className={clsx(styles.h1, className)}>{children}</span>
  ),
  H2: ({className, children}: TypographyProps) => (
    <span className={clsx(styles.h2, className)}>{children}</span>
  ),
  H3: ({className, children}: TypographyProps) => (
    <span className={clsx(styles.h3, className)}>{children}</span>
  ),
  Cta1: ({className, children}: TypographyProps) => (
    <span className={clsx(styles.cta1, className)}>{children}</span>
  ),
  Cta2: ({className, children}: TypographyProps) => (
    <span className={clsx(styles.cta2, className)}>{children}</span>
  ),
  Cta3: ({className, children}: TypographyProps) => (
    <span className={clsx(styles.cta3, className)}>{children}</span>
  ),
};
