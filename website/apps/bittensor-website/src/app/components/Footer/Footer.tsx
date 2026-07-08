'use client';
import {Typography} from '@/app/(pages-without-header)/components/Typography/Typography';
import {Link} from '@raofoundation/ui';
import {useMediaQuery} from 'react-responsive';
import styles from './Footer.module.css';
import {useHamburgerMenuStore} from '@/app/stores/useHamburgerMenuStore';

const communityLinks = [
  {
    label: 'LEARN MORE',
    url: '/about',
    isExternal: false,
  },
  {
    label: 'GITHUB',
    url: 'https://github.com/opentensor',
    isExternal: true,
  },
  {
    label: 'DISCORD',
    url: 'https://discord.gg/qasY3HA9F9',
    isExternal: true,
  },
  {
    label: 'YOUTUBE',
    url: 'https://www.youtube.com/@Opentensor',
    isExternal: true,
  },
];

export const Footer = ({isHamburger = false}) => {
  const isMobile = useMediaQuery({maxWidth: 700});
  const hamburgerMenuStore = useHamburgerMenuStore();

  const menuLinks = isMobile
    ? isHamburger
      ? communityLinks.slice(1)
      : [communityLinks[0]]
    : communityLinks;
  return (
    <footer className={styles.footer}>
      <div className={styles.linkContainer}>
        {menuLinks.map((link) => (
          <Link
            href={link.url}
            isExternal={link.isExternal}
            key={link.url}
            onClick={isHamburger ? hamburgerMenuStore.toggle : () => {}}
          >
            <Typography.ParagraphSmall>{link.label}</Typography.ParagraphSmall>
          </Link>
        ))}
      </div>
    </footer>
  );
};
