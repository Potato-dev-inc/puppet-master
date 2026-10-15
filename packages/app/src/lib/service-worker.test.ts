import { describe, expect, it, vi } from 'vitest';
import { isDesktopAppOrigin } from '../pwa/push-core';
import { setupServiceWorker, type ServiceWorkerDeps } from './service-worker';

function deps(inDesktopApp: boolean, cacheNames: string[] = []) {
  const unregister = vi.fn(async () => true);
  const deleted: string[] = [];
  const register = vi.fn();
  const value: ServiceWorkerDeps = {
    inDesktopApp,
    container: {
      getRegistrations: vi.fn(async () => [{ unregister } as unknown as ServiceWorkerRegistration]),
    },
    cacheStorage: {
      keys: vi.fn(async () => cacheNames),
      delete: vi.fn(async (name: string) => {
        deleted.push(name);
        return true;
      }),
    },
    register,
  };
  return { value, unregister, deleted, register };
}

describe('service worker setup', () => {
  it('registers the worker on the phone and in a browser', async () => {
    const d = deps(false);
    await expect(setupServiceWorker(d.value)).resolves.toBe('registered');
    expect(d.register).toHaveBeenCalledTimes(1);
    expect(d.unregister).not.toHaveBeenCalled();
  });

  it('never registers it in the desktop app, and removes one an older version left with its precache', async () => {
    const d = deps(true, ['workbox-precache-v2-http://tauri.localhost/', 'pm-push-intents']);
    await expect(setupServiceWorker(d.value)).resolves.toBe('removed');
    expect(d.register).not.toHaveBeenCalled();
    expect(d.unregister).toHaveBeenCalledTimes(1);
    expect(d.deleted).toEqual(['workbox-precache-v2-http://tauri.localhost/']);
  });

  it('does nothing where service workers are not supported', async () => {
    const d = deps(false);
    await expect(setupServiceWorker({ ...d.value, container: undefined })).resolves.toBe('unsupported');
    expect(d.register).not.toHaveBeenCalled();
  });

  it('the worker knows the desktop app origin and not the phone one', () => {
    expect(isDesktopAppOrigin(new URL('http://tauri.localhost/'))).toBe(true);
    expect(isDesktopAppOrigin(new URL('tauri://localhost/'))).toBe(true);
    expect(isDesktopAppOrigin(new URL('https://pm.example.trycloudflare.com/'))).toBe(false);
    expect(isDesktopAppOrigin(new URL('http://192.168.1.20:17322/'))).toBe(false);
  });
});
