import { useEffect } from 'react';
import { TERMINAL_AUTHORITY_CHANGED_EVENT } from '../terminal/scaled-viewport';

export interface MobileOrchestratorViewport {
  width: number;
  height: number;
  active: boolean;
}

/** Desktop: sidebar display width + optional mobile PTY authority. */
export function useTerminalAuthorityCss(
  sidebarWidth: number,
  mobileViewport: MobileOrchestratorViewport | null,
): void {
  useEffect(() => {
    const root = document.documentElement;
    root.style.setProperty('--pm-terminal-max-width', `${sidebarWidth}px`);

    const mobileActive = mobileViewport?.active && mobileViewport.width > 0;
    if (mobileActive && mobileViewport) {
      root.style.setProperty('--pm-pty-authority-width', `${mobileViewport.width}px`);
      root.style.setProperty('--pm-mobile-orchestrator-active', '1');
    } else {
      root.style.setProperty('--pm-pty-authority-width', `${sidebarWidth}px`);
      root.style.removeProperty('--pm-mobile-orchestrator-active');
    }

    window.dispatchEvent(new Event(TERMINAL_AUTHORITY_CHANGED_EVENT));

    return () => {
      root.style.removeProperty('--pm-terminal-max-width');
      root.style.removeProperty('--pm-pty-authority-width');
      root.style.removeProperty('--pm-mobile-orchestrator-active');
    };
  }, [sidebarWidth, mobileViewport?.active, mobileViewport?.width]);
}
