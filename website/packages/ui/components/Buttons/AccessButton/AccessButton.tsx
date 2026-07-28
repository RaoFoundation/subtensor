import React, {FC} from 'react';
import {BaseIconButton} from '../IconButton/BaseIconButton';
import {Icon} from '../../Icon/Icon';

type AccessButtonProps = {
  handleOnClick?: () => void;
};

export const AccessButton: FC<AccessButtonProps> = ({handleOnClick}) => {
  return (
    <div>
      <BaseIconButton label='ACCESS' isLogoUp={true} handleOnClick={handleOnClick}>
        <div>
          <Icon.Access />
        </div>
      </BaseIconButton>
    </div>
  );
};
