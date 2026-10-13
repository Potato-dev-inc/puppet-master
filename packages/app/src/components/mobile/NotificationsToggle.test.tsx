// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PushDeps } from '../../lib/push-client';
import { NotificationsToggle } from './NotificationsToggle';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function fakeDeps(over: Partial<PushDeps> = {}): PushDeps {
  return {
    permission: () => 'default',
    requestPermission: vi.fn(async () => 'granted' as NotificationPermission),
    isSubscribed: async () => false,
    subscribe: vi.fn(async () => ({ endpoint: 'https://push.example/x', keys: { p256dh: 'p', auth: 'a' } })),
    unsubscribe: vi.fn(async () => undefined),
    fetchKey: async () => 'KEY',
    sendSubscription: vi.fn(async () => undefined),
    removeSubscription: vi.fn(async () => undefined),
    ...over,
  };
}

describe('NotificationsToggle', () => {
  let host: HTMLDivElement;
  let root: Root;
  beforeEach(() => {
    host = document.createElement('div');
    document.body.appendChild(host);
    root = createRoot(host);
    vi.stubGlobal('Notification', { permission: 'default' });
    Object.defineProperty(window, 'PushManager', { value: class {}, configurable: true });
    Object.defineProperty(navigator, 'serviceWorker', { value: {}, configurable: true });
  });
  afterEach(() => {
    act(() => root.unmount());
    host.remove();
    vi.unstubAllGlobals();
  });

  const box = () => host.querySelector('input[aria-label="Notifications"]') as HTMLInputElement;
  const settle = () => act(async () => { await Promise.resolve(); await Promise.resolve(); });

  it('turns on from a tap: permission, subscribe, send; then off again', async () => {
    const deps = fakeDeps();
    await act(async () => root.render(<NotificationsToggle baseUrl="https://pm.example.com/bridge" bridgeReady deps={deps} />));
    await settle();
    expect(box().checked).toBe(false);
    expect(box().disabled).toBe(false);
    await act(async () => box().click());
    await settle();
    expect(deps.requestPermission).toHaveBeenCalledTimes(1);
    expect(deps.sendSubscription).toHaveBeenCalledWith({ endpoint: 'https://push.example/x', keys: { p256dh: 'p', auth: 'a' } });
    expect(box().checked).toBe(true);
    await act(async () => box().click());
    await settle();
    expect(deps.removeSubscription).toHaveBeenCalled();
    expect(box().checked).toBe(false);
  });

  it('shows the blocked hint and a disabled toggle when permission is denied', async () => {
    vi.stubGlobal('Notification', { permission: 'denied' });
    const deps = fakeDeps({ permission: () => 'denied' });
    await act(async () => root.render(<NotificationsToggle baseUrl="u" bridgeReady deps={deps} />));
    await settle();
    expect(box().disabled).toBe(true);
    expect(host.textContent).toContain('blocked');
  });
});
