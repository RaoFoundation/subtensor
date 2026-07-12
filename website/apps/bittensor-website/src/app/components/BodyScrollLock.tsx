'use client';

import {useEffect} from 'react';
import {useHamburgerMenuStore} from '@/app/stores/useHamburgerMenuStore';
import styles from '../layout.module.css';

/**
 * The root layout is a server component (so the Metadata API works), but the
 * hamburger menu still needs to lock body scroll. This toggles the class from
 * the client without forcing the whole layout tree client-side.
 */
export function BodyScrollLock() {
  const isVisible = useHamburgerMenuStore((state) => state.isVisible);

  useEffect(() => {
    document.body.classList.toggle(styles.no_scroll, isVisible);
    return () => document.body.classList.remove(styles.no_scroll);
  }, [isVisible]);

  return null;
}
