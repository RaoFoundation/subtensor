import React, {FC} from 'react';
import {BaseIconButton} from '../IconButton/BaseIconButton';
import {Icon} from '../../Icon/Icon';

type SendTaoButtonProps = {
  handleOnClick?: () => void;
};

export const SendTaoButton: FC<SendTaoButtonProps> = ({handleOnClick}) => {
  return (
    <div>
      <BaseIconButton label='SEND' isLogoUp={true} handleOnClick={handleOnClick}>
        <div>
          <Icon.SendTao />
        </div>
      </BaseIconButton>
    </div>
  );
};
