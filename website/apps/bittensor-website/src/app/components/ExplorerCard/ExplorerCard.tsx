import type {ExplorerCardData} from '@/data/explorers';
import styles from './ExplorerCard.module.css';

type ExplorerCardProps = {
  card: ExplorerCardData;
};

const imageProps = {
  loading: 'lazy' as const,
  decoding: 'async' as const,
};

export function ExplorerCard({card}: ExplorerCardProps) {
  const leftIcon =
    card.leftIconMaskSrc && card.leftIconSrc ? (
      <span className={styles.leftIconMaskedFrame}>
        <span
          className={styles.leftIconMaskedClip}
          style={{
            WebkitMaskImage: `url(${card.leftIconMaskSrc})`,
            maskImage: `url(${card.leftIconMaskSrc})`,
          }}
        >
          <img
            className={styles.leftIconMaskedArt}
            src={card.leftIconSrc}
            alt=""
            {...imageProps}
          />
        </span>
      </span>
    ) : card.leftIconSrc ? (
      <img className={styles.leftIcon} src={card.leftIconSrc} alt="" {...imageProps} />
    ) : (
      <span className={styles.leftIconSpacer} />
    );

  const isExternal = /^https?:\/\//.test(card.href);

  return (
    <a
      className={styles.card}
      data-id={card.id}
      href={card.href}
      {...(isExternal
        ? {target: '_blank', rel: 'noopener noreferrer', 'aria-label': `Open ${card.name} in a new tab`}
        : {'aria-label': `Open ${card.name}`})}
    >
      <span className={styles.imageFrame}>
        <img className={styles.image} src={card.imageSrc} alt="" {...imageProps} />
      </span>
      <span className={styles.gradient} aria-hidden="true" />
      <span className={styles.overlay} aria-hidden="true" />
      {card.badge ? <span className={styles.badge}>{card.badge}</span> : null}
      <span className={styles.logoBar} aria-hidden="true">
        {leftIcon}
        <img className={styles.rightLogo} src={card.logoSrc} alt="" {...imageProps} />
      </span>
    </a>
  );
}
