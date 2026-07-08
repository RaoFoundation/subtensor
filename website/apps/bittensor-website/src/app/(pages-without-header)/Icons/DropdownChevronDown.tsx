import React from 'react';

export const DropdownChevronUp = ({width = 16, height = 9}) => {
  return (
    <svg
      xmlns='http://www.w3.org/2000/svg'
      width={width}
      height={height}
      viewBox='0 0 16 9'
      fill='none'
    >
      <path
        d='M14 7.49951L8 1.49951L2 7.49951'
        stroke='currentColor'
        strokeWidth='2'
        strokeLinecap='square'
      />
    </svg>
  );
};
