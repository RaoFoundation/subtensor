import React, {FC} from 'react';
import styles from './SectionInView.module.css';
import {Link} from '@raofoundation/ui';

export type SectionInViewProps = {
  links: SectionInViewLink[];
};

export type SectionInViewLink = {
  label: string;
  href: string;
};

export const SectionInView: FC<SectionInViewProps> = ({links}) => {
  return (
    <div className={styles.section_container}>
      {links.map((link, index) => {
        const onClick = () => {
          const href = link.href?.replace('#', '');
          const element = document.getElementById(`${href || ''}`);
          const parentNode = element?.parentNode as HTMLDetailsElement;
          if (parentNode && parentNode.tagName === 'DETAILS') {
            parentNode.open = true;
          }
          if (element) {
            element.scrollIntoView({behavior: 'smooth'});
          }
        };
        const destination = window.location.pathname + link.href;
        return (
          <div key={`${index}parent`}>
            <Link
              href={destination}
              key={index}
              className={styles.link}
              isExternal={false}
              onClick={onClick}
            >
              {link.label}
            </Link>
          </div>
        );
      })}
    </div>
  );
};
