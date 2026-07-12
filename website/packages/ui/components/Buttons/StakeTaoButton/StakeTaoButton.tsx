import React, {FC} from 'react';
import {BaseIconButton} from '../IconButton/BaseIconButton';
import {Icon} from '../../Icon/Icon';

type StakeTaoButtonProps = {
  handleOnClick?: () => void;
};

export const StakeTaoButton: FC<StakeTaoButtonProps> = ({handleOnClick}) => {
  return (
    <div>
      <BaseIconButton label='STAKE' isLogoUp={true} handleOnClick={handleOnClick}>
        <div>
          <Icon.StakeTaoIcon />
        </div>
      </BaseIconButton>
    </div>
  );
};
