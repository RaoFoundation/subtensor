import {FC} from 'react';
import {BaseIconButton} from '../IconButton/BaseIconButton';
import {Icon} from '../../Icon/Icon';

type ImportButtonProps = {
  handleOnClick?: () => void;
};

export const ImportButton: FC<ImportButtonProps> = ({handleOnClick}) => {
  return (
    <div>
      <BaseIconButton label='IMPORT' isLogoUp={true} handleOnClick={handleOnClick}>
        <div>
          <Icon.Import />
        </div>
      </BaseIconButton>
    </div>
  );
};
