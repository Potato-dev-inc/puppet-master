// "Fresh state" for the Settings window. The window is hidden, not destroyed, when you close it, so it
// must reload whenever the settings may have changed behind its back:
//   - `settings://changed`  another window saved something (Rust emits it from `settings_patch`)
//   - `settings://show`     the window was brought back (Rust emits it with the tab to open)
//   - window focus / the page becoming visible again
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export type SettingsRefreshReason = 'changed' | 'show' | 'focus';

export interface SettingsRefreshInfo {
  reason: SettingsRefreshReason;
  /** `settings://show` may name the page to open. */
  tab?: string;
}

function inTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

/** Call `onRefresh` whenever settings may be stale. Returns the unsubscribe function. */
export function subscribeSettingsRefresh(onRefresh: (info: SettingsRefreshInfo) => void): () => void {
  let disposed = false;
  const unlisteners: UnlistenFn[] = [];

  const onFocus = () => onRefresh({ reason: 'focus' });
  const onVisible = () => {
    if (document.visibilityState === 'visible') onRefresh({ reason: 'focus' });
  };
  window.addEventListener('focus', onFocus);
  document.addEventListener('visibilitychange', onVisible);

  if (inTauri()) {
    const add = (p: Promise<UnlistenFn>) =>
      void p.then((un) => (disposed ? un() : unlisteners.push(un))).catch(() => undefined);
    add(listen('settings://changed', () => onRefresh({ reason: 'changed' })));
    add(
      listen<{ tab?: string } | string | null>('settings://show', (event) => {
        const payload = event.payload;
        onRefresh({ reason: 'show', tab: typeof payload === 'string' ? payload : (payload?.tab ?? undefined) });
      }),
    );
  }

  return () => {
    disposed = true;
    window.removeEventListener('focus', onFocus);
    document.removeEventListener('visibilitychange', onVisible);
    unlisteners.forEach((un) => un());
  };
}
