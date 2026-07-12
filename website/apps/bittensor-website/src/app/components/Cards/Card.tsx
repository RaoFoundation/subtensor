import React, {FC} from 'react';
import styles from './Card.module.css';
import {Link, Typography} from '@raofoundation/ui';

type CardProps = {
  index: number;
  title: string;
  description: string;
  action: string;
  actionUrl?: string;
};

export const Card: FC<CardProps> = ({index, title, description, action, actionUrl}) => {
  return (
    <div className={styles.card_container}>
      <p className={styles.action}>{`0${index}`}</p>
      <p className={styles.title}>{title}</p>
      <div className={styles.div}>
        <p className={styles.description}>{description}</p>
      </div>

      {actionUrl ? (
        <Link href={actionUrl}>
          <p className={styles.action}>{action}</p>
        </Link>
      ) : (
        <p className={styles.action}>{action}</p>
      )}
    </div>
  );
};
