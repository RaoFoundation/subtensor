import React, {Suspense} from 'react';
import Image from 'next/image';
import {Link} from '@raofoundation/ui';
import FadeInWrapper from '@/app/components/FadeInWrapper';
import styles from './page.module.css';
import Section1 from './Section1';
import Section2 from './Section2';
import Section3 from './Section3';
import Section4 from './Section4';
import Section5 from './Section5';

const Page = () => {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <p className={styles.paper_title}>
            Bittensor: A Peer&#45;to&#45;Peer Intelligence Market
          </p>
          <p className={styles.subtitle}>Yuma Rao / Dr. Nick / 0xcacti</p>
          <Image
            src='/images/icons/double-tao-logo.svg'
            width={40}
            height={40}
            alt='double tao logo'
          />
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>00/ Abstract</p>
          <p className={styles.abstract_text}>
            In this document, we present Dynamic TAO (DTAO): a market&#45;driven mechanism designed
            to replace Bittensor&apos;s current emission allocation model. In particular, Dynamic
            TAO derives emission values from the market prices of subnet&#45;specific tokens that
            trade against TAO on Constant Product AMMs. This fundamentally transforms the existing
            validator&#45;weighting system. More specifically, by introducing AMM&#45;based emission
            allocation, we extend the subnet valuation system beyond Bittensor&apos;s validator set
            to the rest of the Bittensor ecosystem, including miners and speculators. By replacing
            the current subnet valuation model with an open market, we make the subnet valuation
            process more efficient, increase coordination costs for malicious participants aiming to
            manipulate emissions, and eliminate the apathetic oligarchic voting system
          </p>
          <ul className={styles.unorder_list}>
            <p>This document will be presented in five sections:</p>
            <li>Section 1: Motivation</li>
            <li>Section 2: What is DTAO</li>
            <li>Section 3: Technical Overview</li>
            <li>Section 4: Mathematical Analysis </li>
            <li>Section 5: Appendix</li>
          </ul>
        </section>
        <Section1 />
        <Section2 />
        <Section3 />
        <Section4 />
        <Section5 />
        <span className={styles.paper_link}>
          <Link
            href={
              'https://drive.google.com/file/d/1vkuxOFPJyUyoY6dQzfIWwZm2_XL3AEOx/view?usp=sharing'
            }
            isExternal={true}
          >
            Follow this link for the original version
          </Link>
        </span>
      </FadeInWrapper>
    </Suspense>
  );
};

export default Page;
