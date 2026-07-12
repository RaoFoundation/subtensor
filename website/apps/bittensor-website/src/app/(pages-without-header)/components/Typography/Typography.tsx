import clsx from 'clsx';
import React from 'react';
import styles from './Typography.module.css';

export type TypographyProps = {
  className?: string;
  children?: React.ReactNode;
};

export const Typography = {
  Display1: ({className, children}: TypographyProps) => (
    <span className={clsx(styles.display1, className)}>{children}</span>
  ),
  Display2: ({className, children}: TypographyProps) => (
    <span className={clsx(styles.display2, className)}>{children}</span>
  ),
  Display3: ({className, children}: TypographyProps) => (
    <span className={clsx(styles.display3, className)}>{children}</span>
  ),
  Display4: ({className, children}: TypographyProps) => (
    <span className={clsx(styles.display4, className)}>{children}</span>
  ),
  Display5: ({className, children}: TypographyProps) => (
    <span className={clsx(styles.display5, className)}>{children}</span>
  ),
  Heading1: ({className, children}: TypographyProps) => (
    <span className={clsx(styles.heading1, className)}>{children}</span>
  ),
  Heading2: ({className, children}: TypographyProps) => (
    <span className={clsx(styles.heading2, className)}>{children}</span>
  ),
  Heading3: ({className, children}: TypographyProps) => (
    <span className={clsx(styles.heading3, className)}>{children}</span>
  ),
  Heading4: ({className, children}: TypographyProps) => (
    <span className={clsx(styles.heading4, className)}>{children}</span>
  ),
  ParagraphBig: ({className, children}: TypographyProps) => (
    <span className={clsx(styles.paragraph_big, className)}>{children}</span>
  ),
  Paragraph: ({className, children}: TypographyProps) => (
    <span className={clsx(styles.paragraph, className)}>{children}</span>
  ),
  Code: ({className, children}: TypographyProps) => (
    <span className={clsx(styles.code, className)}>{children}</span>
  ),
  Balance: ({className, children}: TypographyProps) => (
    <span className={clsx(styles.balance, className)}>{children}</span>
  ),
  ParagraphSmall: ({className, children}: TypographyProps) => (
    <span className={clsx(styles.paragraph_small, className)}>{children}</span>
  ),
  ParagraphHafferSmall: ({className, children}: TypographyProps) => (
    <span className={clsx(styles.paragraph_haffer_small, className)}>{children}</span>
  ),
  ParagraphExtraSmall: ({className, children}: TypographyProps) => (
    <span className={clsx(styles.paragraph_extra_small, className)}>{children}</span>
  ),
  Label: ({className, children}: TypographyProps) => (
    <span className={clsx(styles.label, className)}>{children}</span>
  ),
  TaoSymbol: ({className, children}: TypographyProps) => (
    <span className={clsx(styles.tao_amount, className)}>{children}</span>
  ),
  ParagraphBigMono: ({className, children}: TypographyProps) => (
    <span className={clsx(styles.paragraph_big_mono, className)}>{children}</span>
  ),
};
