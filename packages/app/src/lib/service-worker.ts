// The service worker is for the phone PWA only (push, offline shell, deep-link intents). The
// desktop window loads its UI from the app bundle; a service worker there serves the precached
// UI of the version that registered it, so after an update the window could keep showing the
// old screens. Inside the desktop app it is never registered, and one left by an older version
// (0.2.0 and before registered it everywhere) is removed together with its precache.

export const isTauriWindow = (): boolean =>
  typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** Workbox precache names start with this prefix (`workbox-precache-v2-<scope>`). */
const PRECACHE_PREFIX = 'workbox-precache';

export interface ServiceWorkerDeps {
  inDesktopApp: boolean;
  container: Pick<ServiceWorkerContainer, 'getRegistrations'> | undefined;
  cacheStorage: Pick<CacheStorage, 'keys' | 'delete'> | undefined;
  register: () => void | Promise<unknown>;
}

export type ServiceWorkerSetup = 'registered' | 'removed' | 'unsupported';

export async function setupServiceWorker(deps: ServiceWorkerDeps): Promise<ServiceWorkerSetup> {
  if (!deps.container) return 'unsupported';
  if (!deps.inDesktopApp) {
    await deps.register();
    return 'registered';
  }
  const registrations = await deps.container.getRegistrations();
  await Promise.all(registrations.map((registration) => registration.unregister()));
  if (deps.cacheStorage) {
    const names = await deps.cacheStorage.keys();
    await Promise.all(
      names.filter((name) => name.startsWith(PRECACHE_PREFIX)).map((name) => deps.cacheStorage!.delete(name)),
    );
  }
  return 'removed';
}
