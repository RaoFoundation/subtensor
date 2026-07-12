import {AccessButton} from '../AccessButton/AccessButton';
import {ImportButton} from '../ImportButton/ImportButton';
import {CreateButton} from '../CreateButton/CreateButton';
import {SendTaoButton} from '../SendTaoButton/SendTaoButton';
import {ReceiveTaoButton} from '../ReceiveTaoButton/RecieveTaoButton';
import {StakeTaoButton} from '../StakeTaoButton/StakeTaoButton';
import {SearchButton} from '../SearchButton/SearchButton';

export type IconButtonProps = {
  handleOnclick?: () => void;
};

export const IconButton = {
  Empty: (props: IconButtonProps) => <></>,
  Access: (props: IconButtonProps) => <AccessButton handleOnClick={props.handleOnclick} />,
  Import: (props: IconButtonProps) => <ImportButton handleOnClick={props.handleOnclick} />,
  Create: (props: IconButtonProps) => <CreateButton handleOnClick={props.handleOnclick} />,
  SendTao: (props: IconButtonProps) => <SendTaoButton handleOnClick={props.handleOnclick} />,
  ReceiveTao: (props: IconButtonProps) => <ReceiveTaoButton handleOnClick={props.handleOnclick} />,
  StakeTao: (props: IconButtonProps) => <StakeTaoButton handleOnClick={props.handleOnclick} />,
  Search: (props: IconButtonProps) => <SearchButton handleOnClick={props.handleOnclick} />,
};
