import React, {FC} from 'react';
import {BaseIconButton} from '../IconButton/BaseIconButton';
import {Icon} from '../../Icon/Icon';

type ReceiveTaoButtonProps = {
  handleOnClick?: () => void;
};

export const ReceiveTaoButton: FC<ReceiveTaoButtonProps> = ({handleOnClick}) => {
  return (
    <div>
      <BaseIconButton label='RECIEVE' isLogoUp={true} handleOnClick={handleOnClick}>
        <div>
          <Icon.ReceiveTao />
        </div>
      </BaseIconButton>
    </div>
  );
};
