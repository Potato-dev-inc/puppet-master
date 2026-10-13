import { describe, expect, it, vi } from 'vitest';
import { disable, enable, initialState, toggleView, type PushDeps, type PushEnv } from './push-client';

const env: PushEnv = { hasServiceWorker: true, hasPushManager: true, hasNotification: true, isIOS: false, isStandalone: false };

function deps(over: Partial<PushDeps> = {}): PushDeps & { calls: string[] } {
  const calls: string[] = [];
  const log = <T,>(name: string, value: T) => async () => {
    calls.push(name);
    return value;
  };
  return {
    calls,
    permission: () => 'default',
    requestPermission: log('requestPermission', 'granted' as NotificationPermission),
    isSubscribed: log('isSubscribed', false),
    fetchKey: log('fetchKey', 'BKEY'),
    subscribe: async () => {
      calls.push('subscribe');
      return { endpoint: 'https://push.example/x', keys: { p256dh: 'p', auth: 'a' } };
    },
    sendSubscription: async () => {
      calls.push('sendSubscription');
    },
    unsubscribe: log('unsubscribe', undefined),
    removeSubscription: log('removeSubscription', undefined),
    ...over,
  };
}

describe('initial state', () => {
  it('walks the support ladder', () => {
    expect(initialState({ ...env, hasPushManager: false }, 'default', false).kind).toBe('unsupported');
    expect(initialState({ ...env, isIOS: true, isStandalone: false }, 'default', false).kind).toBe('ios-install');
    expect(initialState({ ...env, isIOS: true, isStandalone: true }, 'default', false).kind).toBe('off');
    expect(initialState(env, 'denied', false).kind).toBe('blocked');
    expect(initialState(env, 'granted', true).kind).toBe('on');
    expect(initialState(env, 'granted', false).kind).toBe('off');
    expect(initialState(env, 'default', true).kind).toBe('off');
  });

  it('shows a plain hint for iPhone outside the home screen app and disables the toggle', () => {
    const view = toggleView({ kind: 'ios-install' });
    expect(view.disabled).toBe(true);
    expect(view.hint).toMatch(/Home Screen/);
    expect(toggleView({ kind: 'on' })).toMatchObject({ checked: true, disabled: false });
    expect(toggleView({ kind: 'off' })).toMatchObject({ checked: false, disabled: false });
  });
});

describe('enable', () => {
  it('asks for permission FIRST (user gesture), then key, subscribe, send', async () => {
    const d = deps();
    expect(await enable(d)).toEqual({ kind: 'on' });
    expect(d.calls).toEqual(['requestPermission', 'fetchKey', 'subscribe', 'sendSubscription']);
  });

  it('stops at a denied prompt without touching the PC', async () => {
    const d = deps({ requestPermission: async () => 'denied' });
    expect(await enable(d)).toEqual({ kind: 'blocked' });
    expect(d.calls).toEqual([]);
  });

  it('stays off when the prompt is dismissed', async () => {
    const d = deps({ requestPermission: async () => 'default' });
    expect(await enable(d)).toEqual({ kind: 'off' });
  });

  it('reports a failed subscribe as an error the user can retry', async () => {
    const d = deps({ subscribe: vi.fn(async () => Promise.reject(new Error('push service unreachable'))) });
    const state = await enable(d);
    expect(state.kind).toBe('error');
    expect(toggleView(state)).toMatchObject({ checked: false, disabled: false });
    expect(toggleView(state).hint).toContain('push service unreachable');
  });
});

describe('disable', () => {
  it('removes the PC side and the browser side', async () => {
    const d = deps();
    expect(await disable(d)).toEqual({ kind: 'off' });
    expect(d.calls).toEqual(['removeSubscription', 'unsubscribe']);
  });

  it('still unsubscribes locally when the PC is unreachable', async () => {
    const d = deps({ removeSubscription: async () => Promise.reject(new Error('offline')) });
    expect(await disable(d)).toEqual({ kind: 'off' });
    expect(d.calls).toEqual(['unsubscribe']);
  });
});
