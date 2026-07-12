'use client';

import React, {useEffect} from 'react';
import {Markdown} from '@/app/components/Markdown/Markdown';

import styles from './page.module.css';
import {
  SectionInView,
  SectionInViewLink,
} from '@/app/components/SectionInView/SectionInView';
import {content} from './content';

const Page = () => {
  const [links, setLinks] = React.useState<SectionInViewLink[]>([]);

  let assethPath = ``;

  useEffect(() => {
    window.scrollTo(0, 0);

    const hash = window?.location?.hash.replace('#', '') || '';
    const element = document.getElementById(hash);
    const parentNode = element?.parentNode as HTMLDetailsElement;

    if (parentNode && parentNode.tagName === 'DETAILS') {
      parentNode.open = true;
    }

    if (element) {
      element.scrollIntoView({behavior: 'smooth'});
    }
  });

  return (
    <>
      <div className={styles.page_transition}>
        <Markdown assetsPath={assethPath} setSectionLinks={setLinks}>
          {content}
        </Markdown>
      </div>
      <div className={styles.section_view}>
        <SectionInView links={links} />
      </div>
    </>
  );
};

export default Page;
