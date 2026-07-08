'use client';

import React from 'react';
import {useState} from 'react';
import NextLink from 'next/link';
import Lottie from 'react-lottie';
import clsx from 'clsx';

import styles from './Logo.module.css';

import * as ROUTES from '../routes';

import * as animationData from './logo-animation.json';

export const Logo = () => {
  const [isHovered, setIsHovered] = useState(false);
  const [isPlaying, setIsPlaying] = useState(false);
  const [isPaused, setIsPaused] = useState(false);

  const handleMouseEnter = () => {
    setIsHovered(true);
    setIsPlaying(true);
    setIsPaused(false);
  };

  const handleMouseLeave = () => {
    setIsPaused(false);
    setIsHovered(false);
  };

  const handleAnimationComplete = () => {
    setIsPlaying(false);
  };

  const handleDrawnFrame = (e: any) => {
    // Magic number because the entrance animation
    // doesn't stop exactly in the middle.
    if (e.currentTime > e.totalTime / 3) {
      setIsPaused(true);
    }
  };

  return (
    <div className={styles.logo} onMouseEnter={handleMouseEnter} onMouseLeave={handleMouseLeave}>
      <NextLink href={ROUTES.HOME.linkTo()} scroll={false}>
        <div className={clsx('logo-lottie')}>
          {/* @ts-ignore */}
          <Lottie
            options={{
              loop: false,
              animationData: animationData,
            }}
            height={33}
            isStopped={!isPlaying}
            isPaused={isPaused && isHovered}
            eventListeners={[
              {
                eventName: 'complete',
                callback: handleAnimationComplete,
              },
              {
                eventName: 'drawnFrame',
                callback: handleDrawnFrame,
              },
            ]}
          />
        </div>
      </NextLink>
    </div>
  );
};
