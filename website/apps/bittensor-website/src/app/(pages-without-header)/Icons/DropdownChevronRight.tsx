import React from 'react';

export const DropdownChevronRight = ({width = 10, height = 15}) => {
  return (
    <svg
      xmlns='http://www.w3.org/2000/svg'
      width={width}
      height={height}
      viewBox='0 0 10 15'
      fill='none'
    >
      <path d='M2 13.5L8 7.5L2 1.5' stroke='currentColor' strokeWidth='2' strokeLinecap='square' />
    </svg>
  );
};
