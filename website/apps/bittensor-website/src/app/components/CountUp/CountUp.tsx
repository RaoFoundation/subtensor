'use client';

import React, {FC, useEffect, useRef} from 'react';
import {motion, useMotionValue, useTransform, animate, useInView} from 'framer-motion';

type CountUpProps = {
  value: number;
};
export const CountUp: FC<CountUpProps> = ({value}) => {
  const ref = useRef(null);
  const isInView = useInView(ref, {once: true, amount: 1});

  const count = useMotionValue(0);
  const rounded = useTransform(count, (e) => {
    const round = Math.round(e);
    return round.toLocaleString();
  });

  useEffect(() => {
    count.set(0);
    rounded.set('0');
    const animation = animate(count, value, {duration: 2});

    return animation.stop;
  }, [count, rounded, value, isInView]);

  return <motion.p ref={ref}>{rounded}</motion.p>;
};
