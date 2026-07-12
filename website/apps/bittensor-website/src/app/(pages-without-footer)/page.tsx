'use client';

import {Canvas} from '@/app/components/Canvas/Canvas';
import styles from './page.module.css';

export default function Page() {
  return (
    <div className={styles.container}>
      <div className={styles.e8_canvas}>
        <Canvas />
      </div>
    </div>
  );
}
