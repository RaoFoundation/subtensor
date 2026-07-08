import {useColors} from '@/app/contexts/ThemeContext';
import React from 'react';

export const FreeBalanceIcon = ({width = 17, height = 16}) => {
  const colors = useColors();

  return (
    <svg width={width} height={height} xmlns='http://www.w3.org/2000/svg'>
      <defs>
        <pattern
          id='diagonal-stripes'
          patternUnits='userSpaceOnUse'
          width='4'
          height='4'
          patternTransform='rotate(45)'
        >
          <rect width='1' height='4' fill={colors.textPrimary} />
        </pattern>
      </defs>
      <circle cx={width / 2} cy={height / 2} r={height / 2} fill='url(#diagonal-stripes)' />
    </svg>
  );
};
