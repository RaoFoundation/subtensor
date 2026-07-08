import {useColors} from '@/app/contexts/ThemeContext';
import React from 'react';

export const StakedBalanceIcon = ({width = 18, height = 18}) => {
  const colors = useColors();

  return (
    <svg
      xmlns='http://www.w3.org/2000/svg'
      width={width}
      height={height}
      viewBox='0 0 18 18'
      fill='none'
    >
      <rect x='0.5' y='0.73877' width='16.5227' height='16.5227' fill={colors.textPrimary} />
      <rect
        x='1.78467'
        y='2.02344'
        width='13.9589'
        height='13.9589'
        fill={colors.textPrimary}
        stroke={colors.bgPrimary}
      />
    </svg>
  );
};
