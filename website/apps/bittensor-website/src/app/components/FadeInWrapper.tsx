'use client';
import {motion} from 'framer-motion';
import React from 'react';

interface Props extends React.PropsWithChildren {
  className?: string;
}

export default function FadeInWrapper({className, children}: Props) {
  return (
    <motion.div
      className={className}
      initial={{opacity: 0}}
      animate={{opacity: 1}}
      transition={{duration: 1}}
    >
      {children}
    </motion.div>
  );
}
