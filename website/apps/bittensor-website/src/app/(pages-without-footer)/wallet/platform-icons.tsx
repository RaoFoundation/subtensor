import type {FC} from 'react';
import type {PlatformKind} from './platform-kinds';
import styles from './platform-icons.module.css';

const BASE = '/wallet/figma-platform';

function AppleIcon() {
  return (
    <div className={`${styles.stack} ${styles.stackNoClip}`}>
      <img src={`${BASE}/apple.svg`} alt="" className={styles.apple} width={12} height={12} aria-hidden draggable={false} />
    </div>
  );
}

function GooglePlayIcon() {
  return (
    <div className={`${styles.stack} ${styles.stackNoClip}`}>
      <img src={`${BASE}/google-play.svg`} alt="" className={styles.googlePlay} aria-hidden draggable={false} />
    </div>
  );
}

function ChromeIcon() {
  return (
    <div className={styles.stack}>
      <img src={`${BASE}/chrome.svg`} alt="" className={styles.chrome} aria-hidden draggable={false} />
    </div>
  );
}

function FirefoxIcon() {
  return (
    <div className={styles.stack}>
      <img src={`${BASE}/firefox.svg`} alt="" className={styles.fx} aria-hidden draggable={false} />
    </div>
  );
}

function ExternalIcon() {
  return (
    <div className={styles.stack}>
      <img src={`${BASE}/external.svg`} alt="" className={styles.ex1} width={12} height={12} aria-hidden draggable={false} />
    </div>
  );
}

function LedgerIcon() {
  return (
    <div className={styles.stack}>
      <img src={`${BASE}/ledger.svg`} alt="" className={styles.ledger} width={12} height={12} aria-hidden draggable={false} />
    </div>
  );
}

function CliIcon() {
  return (
    <div className={styles.stack}>
      <img src={`${BASE}/cli.svg`} alt="" className={styles.cli} width={12} height={12} aria-hidden draggable={false} />
    </div>
  );
}

function TangemIcon() {
  return (
    <div className={styles.stack}>
      <img src={`${BASE}/tangem.svg`} alt="" className={styles.tang} width={12} height={12} aria-hidden draggable={false} />
    </div>
  );
}

function PolkadotVaultIcon() {
  return (
    <div className={styles.stack}>
      <img src={`${BASE}/polkadot.svg`} alt="" className={styles.pv} width={12} height={12} aria-hidden draggable={false} />
    </div>
  );
}

export const PlatformIcon: FC<{kind: PlatformKind}> = ({kind}) => {
  switch (kind) {
    case 'apple':
      return <AppleIcon />;
    case 'googlePlay':
      return <GooglePlayIcon />;
    case 'chrome':
      return <ChromeIcon />;
    case 'firefox':
      return <FirefoxIcon />;
    case 'external':
      return <ExternalIcon />;
    case 'ledger':
      return <LedgerIcon />;
    case 'cli':
      return <CliIcon />;
    case 'tangem':
      return <TangemIcon />;
    case 'polkadotVault':
      return <PolkadotVaultIcon />;
    default:
      return null;
  }
};
