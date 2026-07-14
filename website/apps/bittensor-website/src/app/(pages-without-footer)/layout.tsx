import React from 'react';
import {Header} from '../components/Header/Header';
import styles from './layout.module.css';

export default function LayoutWithoutFooter({children}: {children: React.ReactNode}) {
  return (
    <div className={styles.container}>
      <Header />
      {children}
    </div>
  );
}
