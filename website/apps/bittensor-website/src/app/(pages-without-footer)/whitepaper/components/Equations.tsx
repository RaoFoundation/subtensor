import React, {FC} from 'react';
// @ts-ignore
import {InlineMath, BlockMath} from 'react-katex';
import clsx from 'clsx';
import styles from './Equations.module.css';
import 'katex/dist/katex.min.css';

type EquationProps = {
  equNo: number;
  equ: string;
  minify?: boolean;
};

export const Equations: FC<EquationProps> = ({equNo, equ, minify}) => {
  if (minify === undefined) minify = false;
  return (
    <div className={styles.main}>
      <h2 className={styles.equ_no}>(`{equNo}`)</h2>
      <div className={clsx(minify ? styles.text_minify : styles.non_minified)}>
        <BlockMath>{equ}</BlockMath>
      </div>
    </div>
  );
};
