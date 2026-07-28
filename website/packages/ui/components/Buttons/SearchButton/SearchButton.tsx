import React, {FC} from 'react';
import {BaseIconButton} from '../IconButton/BaseIconButton';
import {Icon} from '../../Icon/Icon';

type SearchButtonProps = {
  handleOnClick?: () => void;
};

export const SearchButton: FC<SearchButtonProps> = ({handleOnClick}) => {
  return (
    <div>
      <BaseIconButton label='SEARCH' isLogoUp={true} handleOnClick={handleOnClick}>
        <div>
          <Icon.Settings />
        </div>
      </BaseIconButton>
    </div>
  );
};
