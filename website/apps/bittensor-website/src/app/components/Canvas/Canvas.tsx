'use client';

import {useEffect, useRef} from 'react';

import {initCanvas, unmountCanvas} from './initCanvas';

import styles from './Canvas.module.css';

export const Canvas = () => {
  const canvasRef = useRef(null);
  useEffect(() => {
    const canvas = canvasRef?.current;
    if (!canvas) return;

    initCanvas(canvas);

    return () => unmountCanvas();
  }, [canvasRef]);

  return (
    <div className={styles.canvas}>
      <canvas ref={canvasRef} width='1500' height='1500' />
    </div>
  );
};
