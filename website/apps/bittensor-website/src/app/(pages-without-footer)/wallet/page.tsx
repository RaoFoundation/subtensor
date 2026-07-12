'use client';

import FadeInWrapper from '@/app/components/FadeInWrapper';
import Link from 'next/link';
import type {MouseEvent, ReactNode} from 'react';
import {useCallback, useEffect, useRef, useState} from 'react';
import type {PlatformKind} from './platform-kinds';
import {PlatformIcon} from './platform-icons';
import styles from './page.module.css';

const TAOSTATS_MOBILE_ART = '/wallet/taostats-mobile-label.svg';
const TAOSTATS_BROWSER_ART = '/wallet/taostats-browser-label.svg';

/** How long the high-contrast chip cue stays on (ms). Ease-out uses 0.4s on chips in page.module.css. */
const TAOSTATS_PROMPT_HOLD_MS = 1000;

/** Card grey-500 stroke cue after hit-area click; border eases out over 0.4s in page.module.css. */
const CARD_BORDER_ACCENT_HOLD_MS = 1000;

type PlatformTarget = {
  kind: PlatformKind;
};

type TaostatsActions = {
  mobileHref: string;
  browserHref: string;
};

type StandardWalletCard = {
  title: string;
  description: ReactNode;
  platforms: PlatformTarget[];
  primaryHref: string;
  primaryAriaLabel: string;
};

type TaostatsWalletCard = {
  title: string;
  description: ReactNode;
  platforms: PlatformTarget[];
  taostatsActions: TaostatsActions;
};

type WalletCardConfig = StandardWalletCard | TaostatsWalletCard;

function WalletCard({card}: {card: WalletCardConfig}) {
  const taostatsActions = 'taostatsActions' in card ? card.taostatsActions : undefined;
  const isTaostatsCard = taostatsActions !== undefined;

  const [taostatsPromptFlash, setTaostatsPromptFlash] = useState(false);
  const taostatsPromptTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const [cardBorderAccentFlash, setCardBorderAccentFlash] = useState(false);
  const cardBorderAccentTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const triggerCardBorderAccent = useCallback(() => {
    if (cardBorderAccentTimerRef.current) {
      clearTimeout(cardBorderAccentTimerRef.current);
    }
    setCardBorderAccentFlash(true);
    cardBorderAccentTimerRef.current = setTimeout(() => {
      setCardBorderAccentFlash(false);
      cardBorderAccentTimerRef.current = null;
    }, CARD_BORDER_ACCENT_HOLD_MS);
  }, []);

  const triggerTaostatsPromptFlash = useCallback(() => {
    if (!taostatsActions) {
      return;
    }
    if (taostatsPromptTimerRef.current) {
      clearTimeout(taostatsPromptTimerRef.current);
    }
    setTaostatsPromptFlash(true);
    taostatsPromptTimerRef.current = setTimeout(() => {
      setTaostatsPromptFlash(false);
      taostatsPromptTimerRef.current = null;
    }, TAOSTATS_PROMPT_HOLD_MS);
  }, [taostatsActions]);

  const onTaostatsCardHitClick = useCallback(
    (e: MouseEvent<HTMLButtonElement>) => {
      /* No cardBorderAccent — outer stroke stays hover/default; only MOBILE/BROWSER cue. */
      triggerTaostatsPromptFlash();
      /* Blur: mouse/fine (detail>0) avoids :focus-within pin; touch/coarse (detail often 0) avoids
         the same. Skip for keyboard (detail 0 + fine pointer) so Tab focus ring stays predictable. */
      const coarse =
        typeof window !== 'undefined' && window.matchMedia('(pointer: coarse)').matches;
      if (e.detail > 0 || coarse) {
        e.currentTarget.blur();
      }
    },
    [triggerTaostatsPromptFlash],
  );

  useEffect(() => {
    return () => {
      if (taostatsPromptTimerRef.current) {
        clearTimeout(taostatsPromptTimerRef.current);
      }
      if (cardBorderAccentTimerRef.current) {
        clearTimeout(cardBorderAccentTimerRef.current);
      }
    };
  }, []);

  const articleClassName = [
    styles.card,
    styles.cardUnifiedHit,
    !isTaostatsCard && cardBorderAccentFlash ? styles.cardBorderAccentFlash : '',
    isTaostatsCard && taostatsPromptFlash ? styles.taostatsPromptFlash : '',
  ]
    .filter(Boolean)
    .join(' ');

  return (
    <article className={articleClassName}>
      <div className={styles.cardHeader}>
        <h3 className={styles.cardTitle}>{card.title}</h3>
        <div className={styles.iconRail}>
          {card.platforms.map((p, index) => (
            <span
              key={`${card.title}-${p.kind}-${index}`}
              className={styles.iconDecor}
              aria-hidden>
              <PlatformIcon kind={p.kind} />
            </span>
          ))}
        </div>
      </div>
      <div className={styles.cardBody}>
        <p className={styles.cardDescription}>{card.description}</p>
        {taostatsActions ? (
          <div className={styles.taostatsActions}>
            <Link
              href={taostatsActions.mobileHref}
              target="_blank"
              rel="noreferrer noopener"
              className={`${styles.taostatsLink} ${styles.taostatsLinkMobile}`}
              aria-label="Taostats Wallet — open mobile app">
              <span className={styles.taostatsLabelWrap}>
                <img
                  src={TAOSTATS_MOBILE_ART}
                  alt=""
                  width={44}
                  height={9}
                  draggable={false}
                  aria-hidden
                  className={styles.taostatsArtDefault}
                />
              </span>
            </Link>
            <Link
              href={taostatsActions.browserHref}
              target="_blank"
              rel="noreferrer noopener"
              className={`${styles.taostatsLink} ${styles.taostatsLinkBrowser}`}
              aria-label="Taostats Wallet — open browser extension">
              <span className={styles.taostatsLabelWrap}>
                <img
                  src={TAOSTATS_BROWSER_ART}
                  alt=""
                  width={51}
                  height={9}
                  draggable={false}
                  aria-hidden
                  className={styles.taostatsArtBrowser}
                />
              </span>
            </Link>
          </div>
        ) : null}
      </div>
      {isTaostatsCard ? (
        <button
          type="button"
          className={styles.cardHitArea}
          aria-label="Taostats Wallet — choose Mobile app or Browser extension"
          onClick={onTaostatsCardHitClick}>
          <span className={styles.visuallyHidden}>{card.title}</span>
        </button>
      ) : (
        <Link
          href={(card as StandardWalletCard).primaryHref}
          target="_blank"
          rel="noreferrer noopener"
          className={styles.cardHitArea}
          aria-label={(card as StandardWalletCard).primaryAriaLabel}
          onClick={triggerCardBorderAccent}>
          <span className={styles.visuallyHidden}>{card.title}</span>
        </Link>
      )}
    </article>
  );
}

const walletsSectionCards: WalletCardConfig[] = [
  {
    title: 'Crucible Wallet',
    primaryHref: 'https://cruciblelabs.com/',
    primaryAriaLabel: 'Crucible Wallet — open website',
    description:
      'Chrome extension wallet for Bittensor, with staking tools and Ledger support',
    platforms: [{kind: 'chrome'}, {kind: 'external'}],
  },
  {
    title: 'TAO.com Wallet',
    primaryHref: 'https://www.tao.com',
    primaryAriaLabel: 'TAO.com Wallet — open website',
    description: 'Mobile wallet for managing TAO, staking, and everyday transactions',
    platforms: [{kind: 'apple'}],
  },
  {
    title: 'Taostats Wallet',
    description: 'TAO transfers, staking + Bittensor analytics',
    platforms: [{kind: 'apple'}, {kind: 'googlePlay'}, {kind: 'chrome'}, {kind: 'external'}],
    taostatsActions: {
      mobileHref: 'https://taostats.io/app',
      browserHref: 'https://taostats.io/bittensor-chrome-wallet',
    },
  },
  {
    title: 'Talisman Wallet',
    primaryHref: 'https://www.talisman.xyz/',
    primaryAriaLabel: 'Talisman Wallet — open website',
    description: 'Open-source, self-custodial, multi-chain wallet supporting Bittensor',
    platforms: [{kind: 'googlePlay'}, {kind: 'chrome'}, {kind: 'firefox'}, {kind: 'external'}],
  },
  {
    title: 'Nova Wallet',
    primaryHref: 'https://novawallet.io/',
    primaryAriaLabel: 'Nova Wallet — open website',
    description: 'Mobile wallet for managing TAO, staking, and everyday transactions',
    platforms: [{kind: 'apple'}, {kind: 'googlePlay'}, {kind: 'ledger'}],
  },
  {
    title: 'Subwallet',
    primaryHref: 'https://www.subwallet.app/',
    primaryAriaLabel: 'Subwallet — open website',
    description: 'Flexible wallet for managing TAO on mobile and browser for staking and transfers',
    platforms: [{kind: 'apple'}, {kind: 'googlePlay'}, {kind: 'chrome'}, {kind: 'firefox'}, {kind: 'ledger'}],
  },
  {
    title: 'Polkadot.js',
    primaryHref: 'https://polkadot.js.org/',
    primaryAriaLabel: 'Polkadot.js — open website',
    description: (
      <>
        Developer-grade interface for advanced
        <br />
        Bittensor interactions
      </>
    ),
    platforms: [{kind: 'chrome'}, {kind: 'external'}],
  },
  {
    title: 'Bittensor CLI',
    primaryHref: 'https://github.com/RaoFoundation/btcli',
    primaryAriaLabel: 'Bittensor CLI — open GitHub repository',
    description: 'Command-line interface for managing wallets, staking, and on-chain interactions',
    platforms: [{kind: 'cli'}],
  },
];

const coldStorageSectionCards: WalletCardConfig[] = [
  {
    title: 'Ledger',
    primaryHref: 'https://support.ledger.com/article/11228984669085-zd',
    primaryAriaLabel: 'Ledger — TAO support article on Ledger Help',
    description: 'Hardware wallet for secure TAO storage, supported via compatible Bittensor wallets',
    platforms: [{kind: 'ledger'}],
  },
  {
    title: 'Polkadot Vault',
    primaryHref: 'https://signer.parity.io/#about',
    primaryAriaLabel: 'Polkadot Vault — open website',
    description: 'Offline, air-gapped signer for cold storage using an old phone and QR-based signing',
    platforms: [{kind: 'polkadotVault'}],
  },
  {
    title: 'Tangem Wallet',
    primaryHref: 'https://tangem.com/',
    primaryAriaLabel: 'Tangem Wallet — open website',
    description: 'Tap-based hardware wallet, partial support (TAO transfers only)',
    platforms: [{kind: 'tangem'}],
  },
];

export default function Page() {
  return (
    <FadeInWrapper className={styles.page}>
      <div className={styles.mainStack}>
        <section className={styles.section} aria-labelledby="wallets-heading">
          <div className={styles.titleBlock}>
            <p id="wallets-heading" className={styles.title}>
              Wallets
            </p>
            <p className={styles.subtitle}>
              THESE ARE COMMONLY USED THIRD-PARTY WALLET OPTIONS FOR THE BITTENSOR ECOSYSTEM. AS WITH ALL
              DECENTRALIZED TOOLS, WE REMIND YOU TO PERFORM YOUR OWN RESEARCH AND PRACTICE VIGILANT SECURITY
              WHEN MANAGING YOUR ASSETS.
            </p>
          </div>
          <div className={styles.cardGrid}>
            {walletsSectionCards.map((card) => (
              <WalletCard key={card.title} card={card} />
            ))}
          </div>
        </section>

        <section className={styles.section} aria-labelledby="cold-storage-heading">
          <div className={styles.titleBlock}>
            <p id="cold-storage-heading" className={styles.title}>
              Cold Storage
            </p>
            <p className={styles.subtitle}>
              These are commonly used third-party cold storage options for keeping your keys separate from your
              primary device. We do not control, audit, or guarantee their security, compatibility, or suitability
              for your use case.
            </p>
          </div>
          <div className={styles.cardGrid}>
            {coldStorageSectionCards.map((card) => (
              <WalletCard key={card.title} card={card} />
            ))}
          </div>
        </section>
      </div>
    </FadeInWrapper>
  );
}
