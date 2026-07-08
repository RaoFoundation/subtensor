'use client';

import React from 'react';
import styles from './Sections.module.css';
import {WHITEPAPER_URL} from '@/app/config';
import {Card} from '../Cards/Card';
import {SubtensorInfo} from '../SubtensorInfo/SubtensorInfo';
import {useQuery} from '@tanstack/react-query';
import {getStats} from './api/getStats';

const cardContent = [
  {
    id: 1,
    title: 'Open',
    description:
      'The protocol is accessible to anyone, anytime, anywhere. Join us and start building.',
    action: 'Get Started',
    actionUrl: '/docs',
  },
  {
    id: 2,
    title: 'Incentivized',
    description:
      'TAO serves as both reward for intelligence contributions and access to the network.',
    action: 'Create a Wallet',
    actionUrl: '/wallet',
  },
  {
    id: 3,
    title: 'Decentralized',
    description:
      'Structural dispersion doesn’t just improve the distribution model, it optimizes training.',
    action: 'Read the whitepaper',
    actionUrl: WHITEPAPER_URL,
  },
  {
    id: 4,
    title: 'User-Governed',
    description:
      'The project is community-driven. Administrative power is relinquished as much as possible.',
    action: 'Join the discord',
    actionUrl: 'https://discord.gg/qasY3HA9F9',
  },
];

const difference = new Date().getTime() - new Date('Wed Nov 03 2021 21:46:55 GMT+0200').getTime();
const days = Math.ceil(difference / (1000 * 3600 * 24));

const subTensorInfoContent = {
  daysRunning: days,
  activeNodes: 4096,
  taoStakes: 3876642,
};

const Sections = () => {
  const {data: stats, isLoading} = useQuery({
    queryKey: ['get-stats'],
    queryFn: getStats,
    staleTime: 1000 * 60,
  });

  const taoStaked = !stats?.totalStaked ? 0 : stats?.totalStaked;

  return (
    <div>
      <section>
        <div className={styles.section_3_div}>
          {cardContent.map((card) => (
            <Card
              key={`card-${card.id}`}
              index={card.id}
              title={card.title}
              description={card.description}
              action={card.action}
              actionUrl={card.actionUrl}
            />
          ))}
        </div>
      </section>
      <section className={styles.section_1}>
        <div className={styles.section_1_text}>
          <div>Internet-scale machine learning</div>
        </div>
      </section>
      <section className={styles.section_4}>
        <SubtensorInfo
          isLoading={isLoading}
          daysRunning={subTensorInfoContent.daysRunning}
          activeNodes={subTensorInfoContent.activeNodes}
          taoStaked={taoStaked}
        />
      </section>
      <section className={styles.section_2}>
        <div className={styles.section_2_div}>
          <img src='/images/about-page-image.webp' alt='tech' className={styles.image} />
        </div>
      </section>
      <section className={styles.section_5}>
        <div className={styles.section_5_text}>
          <div>Incentivizing intelligence.</div>
        </div>
      </section>
    </div>
  );
};

export default Sections;
