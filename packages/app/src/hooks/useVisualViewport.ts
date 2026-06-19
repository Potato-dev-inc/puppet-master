import { useEffect } from 'react';

/** Keep layout pinned to the visible viewport so the keyboard does not cover content. */
export function useVisualViewportSync(enabled = true): void {
  useEffect(() => {
    if (!enabled || typeof window === 'undefined') return;

    const viewport = window.visualViewport;
    if (!viewport) return;

    const sync = (): void => {
      const keyboardInset = Math.max(0, window.innerHeight - viewport.offsetTop - viewport.height);
      document.documentElement.style.setProperty('--pm-keyboard-inset', `${keyboardInset}px`);
      document.documentElement.style.setProperty('--pm-visual-viewport-height', `${viewport.height}px`);
      document.documentElement.style.setProperty('--pm-visual-viewport-offset-top', `${viewport.offsetTop}px`);
    };

    sync();
    viewport.addEventListener('resize', sync);
    viewport.addEventListener('scroll', sync);
    window.addEventListener('orientationchange', sync);

    return () => {
      viewport.removeEventListener('resize', sync);
      viewport.removeEventListener('scroll', sync);
      window.removeEventListener('orientationchange', sync);
      document.documentElement.style.removeProperty('--pm-keyboard-inset');
      document.documentElement.style.removeProperty('--pm-visual-viewport-height');
      document.documentElement.style.removeProperty('--pm-visual-viewport-offset-top');
    };
  }, [enabled]);
}
