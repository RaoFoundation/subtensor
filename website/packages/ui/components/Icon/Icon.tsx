import BittensorLogoIcon from '../../assets/BittensorLogo.svg';
import AccessIcon from '../../assets/icons/access-icon.svg';
import AppStoreDownloadIcon from '../../assets/icons/appstore-download-icon.svg';
import BackArrowIcon from '../../assets/icons/back-arrow-icon.svg';
import BackIcon from '../../assets/icons/back-icon.svg';
import CheckMarkIcon from '../../assets/icons/checkmark-icon.svg';
import ChromeStoreDownloadIcon from '../../assets/icons/chromestore-download-icon.svg';
import CloseIcon from '../../assets/icons/closeicon.svg';
import CopyIcon from '../../assets/icons/copy-icon.svg';
import CopiedIcon from '../../assets/icons/copied-icon.svg';
import CreateIcon from '../../assets/icons/create-icon.svg';
import DropArrowIcon from '../../assets/icons/down-arrow-icon.svg';
import DownArrowIcon from '../../assets/icons/down-arrow.svg';
import ForwardArrowIcon from '../../assets/icons/forward-arrow-icon.svg';
import HamburgerIcon from '../../assets/icons/hamburgericon.svg';
import ImportIcon from '../../assets/icons/import-icon.svg';
import LogoIcon from '../../assets/icons/logo.svg';
import MenuIcon from '../../assets/icons/menu-icon.svg';
import ReceiveTaoIcon from '../../assets/icons/receive-tao-icon.svg';
import SendTaoIcon from '../../assets/icons/send-tao-icon.svg';
import SendIcon from '../../assets/icons/sendicon.svg';
import SettingsIcon from '../../assets/icons/settings.svg';
import StakeTaoIcon from '../../assets/icons/stake-tao-icon.svg';
import TaoLogoIconLg from '../../assets/icons/taologo-lg.svg';
import TaoLogoIcon from '../../assets/icons/taologo.svg';
import UpArrowIcon from '../../assets/icons/up-arrow.svg';

export type IconProps = {};

export const Icon = {
  Empty: (props: IconProps) => <></>,
  Logo: (props: IconProps) => <LogoIcon />,
  Settings: (props: IconProps) => <SettingsIcon />,
  DownArrow: (props: IconProps) => <DownArrowIcon />,
  UpArrow: (props: IconProps) => <UpArrowIcon />,
  Send: (props: IconProps) => <SendIcon />,
  Hamburger: (props: IconProps) => <HamburgerIcon />,
  Close: (props: IconProps) => <CloseIcon />,
  TaoLogo: (props: IconProps) => <TaoLogoIcon />,
  TaoLogoLg: (props: IconProps) => <TaoLogoIconLg />,
  BittensorLogo: (props: IconProps) => <BittensorLogoIcon />,
  SendTao: (props: IconProps) => <SendTaoIcon />,
  ReceiveTao: (props: IconProps) => <ReceiveTaoIcon />,
  Copy: (props: IconProps) => <CopyIcon />,
  Copied: (props: IconProps) => <CopiedIcon />,
  Access: (props: IconProps) => <AccessIcon />,
  Import: (props: IconProps) => <ImportIcon />,
  Create: (props: IconProps) => <CreateIcon />,
  Menu: (props: IconProps) => <MenuIcon />,
  Back: (props: IconProps) => <BackIcon />,
  StakeTaoIcon: (props: IconProps) => <StakeTaoIcon />,
  BackArrow: (props: IconProps) => <BackArrowIcon />,
  ForwardArrow: (props: IconProps) => <ForwardArrowIcon />,
  BackDrop: (props: IconProps) => <DropArrowIcon />,
  CheckMark: (props: IconProps) => <CheckMarkIcon />,
  AppStoreDownload: (props: IconProps) => <AppStoreDownloadIcon />,
  ChromeStoreDownload: (props: IconProps) => <ChromeStoreDownloadIcon />,
};
