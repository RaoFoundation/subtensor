// import {Link as BaseLink} from '@raofoundation/ui';
import {ReactNode} from 'react';
import styles from './Markdown.module.css';
import Link from 'next/link';

export type Props = {
  href?: string;
  children: ReactNode;
  [x: string]: any;
};

export const slugify = (str: string) => {
  str = str.replace(/^\s+|\s+$/g, '');
  str = str.toLowerCase();
  str = str
    .replace(/[^a-z0-9 -]/g, '')
    .replace(/\s+/g, '-')
    .replace(/-+/g, '-');
  return str;
};

export const BaseLink = ({href, isExternal, children}: Props) => {
  let onClick = undefined;
  let destination;

  if (href?.startsWith('#')) {
    // If is an anchor link, scroll to the element instead of redirect.
    // This doesn't work out of the box because of the NextJS13 bug
    // described here https://github.com/vercel/next.js/issues/44295
    // (but the solution in the bug description is awful so we do it
    // this way)
    onClick = () => {
      href = href?.replace('#', '');
      const element = document.getElementById(`${href || ''}`);
      if (element) {
        element.scrollIntoView({behavior: 'smooth'});
      }
    };
    destination = window.location.pathname + href;
  } else {
    if (href && href.startsWith('http')) {
      destination = href;
      isExternal = true;
    } else {
      destination = `/documentation/${href}`;
    }
  }
  return (
    <Link href={destination || ''} onClick={onClick} className={styles.link_wrapper}>
      <span className={styles.link}>{children}</span>
    </Link>
  );
};

export const createImage = (assetsPath: string) => {
  const Image = ({src, alt, children, ...props}: Props) => {
    return <img src={`${assetsPath}${src}`} className={styles.image} alt={alt} />;
  };

  return Image;
};

export const H1 = ({node, ...props}: Props) => {
  return <div className={styles.h1} {...props} />;
};

export const H2 = ({node, ...props}: Props) => {
  return <div className={styles.h2} {...props} />;
};

export const H3 = ({node, ...props}: Props) => {
  return <div className={styles.h3} id={slugify(props?.children?.toString() || '')} {...props} />;
};

export const H4 = ({node, ...props}: Props) => {
  return <div className={styles.h4} id={slugify(props?.children?.toString() || '')} {...props} />;
};

export const CodeEl = ({node, inline, ...props}: Props) => {
  return <span className={styles.code} {...props} />;
};

export const Li = ({node, ...props}: Props) => {
  const {ordered, ...restProps} = props;
  return <li className={styles.li} {...restProps} />;
};

export const Ul = ({node, ordered, ...props}: Props) => {
  return <ul className={styles.ul} {...props} />;
};

export const Ol = ({node, ...props}: Props) => {
  const {ordered, ...restProps} = props;
  return <ol className={styles.ol} {...restProps} />;
};

export const Text = ({node, ...props}: Props) => {
  return <p className={styles.text} {...props} />;
};

export const Bold = ({node, ...props}: Props) => {
  return <span className={styles.bold} {...props} />;
};

export const Table = ({node, ...props}: Props) => {
  return <table className={styles.tables} {...props} />;
};

export const Accordion = ({node, ...props}: Props) => (
  <details className={styles.text}>
    <summary className={styles.h3}>{props.title}</summary>
    {props.children}
  </details>
);
