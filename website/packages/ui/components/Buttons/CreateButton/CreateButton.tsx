import React, {FC} from 'react';
import {BaseIconButton} from '../IconButton/BaseIconButton';
import {Icon} from '../../Icon/Icon';

type CreateButtonProps = {
  handleOnClick?: () => void;
};

export const CreateButton: FC<CreateButtonProps> = ({handleOnClick}) => {
  return (
    <div>
      <BaseIconButton label='CREATE' isLogoUp={true} handleOnClick={handleOnClick}>
        <div>
          <Icon.Create />
        </div>
      </BaseIconButton>
    </div>
  );
};
