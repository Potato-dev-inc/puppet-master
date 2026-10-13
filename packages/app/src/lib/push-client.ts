// Phone-side push: the "Notifications" toggle as a small state machine, plus the real wiring
// (service worker, PushManager, bridge routes). The machine is dependency-injected so it is
// tested without a browser.
import { ngrokRequestHeaders } from './bridge-ngrok';
import { mergeBridgeHeaders } from './mobile-pairing-auth';

export interface PushEnv {
  hasServiceWorker: boolean;
  hasPushManager: boolean;
  hasNotification: boolean;
  isIOS: boolean;
  /** Running as an installed home-screen app. */
  isStandalone: boolean;
}

export type ToggleState =
  | { kind: 'unsupported' }
  | { kind: 'ios-install' }
  | { kind: 'blocked' }
  | { kind: 'off' }
  | { kind: 'working' }
  | { kind: 'on' }
  | { kind: 'error'; message: string };

export interface ToggleView {
  checked: boolean;
  disabled: boolean;
  /** Plain-words line under the toggle. */
  hint: string | null;
}

export function toggleView(state: ToggleState): ToggleView {
  switch (state.kind) {
    case 'unsupported':
      return { checked: false, disabled: true, hint: 'This browser cannot show notifications from the web app.' };
    case 'ios-install':
      return {
        checked: false,
        disabled: true,
        hint: 'On iPhone, add Puppet Master to your Home Screen first (Share, then Add to Home Screen), then open it from there.',
      };
    case 'blocked':
      return { checked: false, disabled: true, hint: 'Notifications are blocked for this app. Allow them in your phone settings.' };
    case 'working':
      return { checked: false, disabled: true, hint: null };
    case 'on':
      return { checked: true, disabled: false, hint: 'You will get a notification when an agent needs you, errors, or finishes a run. Previews may show the command on your lock screen.' };
    case 'error':
      return { checked: false, disabled: false, hint: state.message };
    default:
      return { checked: false, disabled: false, hint: 'Get a notification when an agent needs you. Previews may show the command on your lock screen.' };
  }
}

/** The state before the user touches anything. */
export function initialState(env: PushEnv, permission: NotificationPermission, subscribed: boolean): ToggleState {
  if (env.isIOS && !env.isStandalone) return { kind: 'ios-install' };
  if (!env.hasServiceWorker || !env.hasPushManager || !env.hasNotification) return { kind: 'unsupported' };
  if (permission === 'denied') return { kind: 'blocked' };
  return permission === 'granted' && subscribed ? { kind: 'on' } : { kind: 'off' };
}

export interface BrowserSubscription {
  endpoint: string;
  keys: { p256dh: string; auth: string };
}

export interface PushDeps {
  permission: () => NotificationPermission;
  requestPermission: () => Promise<NotificationPermission>;
  isSubscribed: () => Promise<boolean>;
  subscribe: (applicationServerKey: string) => Promise<BrowserSubscription>;
  unsubscribe: () => Promise<void>;
  fetchKey: () => Promise<string>;
  sendSubscription: (sub: BrowserSubscription) => Promise<void>;
  removeSubscription: () => Promise<void>;
}

/** Turn notifications on. MUST be called straight from the tap: the permission prompt is the
 *  first thing it does, before any other await, or browsers refuse to show it. */
export async function enable(deps: PushDeps): Promise<ToggleState> {
  try {
    const permission = await deps.requestPermission();
    if (permission === 'denied') return { kind: 'blocked' };
    if (permission !== 'granted') return { kind: 'off' };
    const key = await deps.fetchKey();
    const sub = await deps.subscribe(key);
    await deps.sendSubscription(sub);
    return { kind: 'on' };
  } catch (e) {
    return { kind: 'error', message: e instanceof Error ? `Could not turn on notifications: ${e.message}` : 'Could not turn on notifications.' };
  }
}

export async function disable(deps: PushDeps): Promise<ToggleState> {
  try {
    await deps.removeSubscription();
  } catch {
    /* the PC may be unreachable; unsubscribing locally still stops pushes at the browser */
  }
  try {
    await deps.unsubscribe();
    return { kind: 'off' };
  } catch (e) {
    return { kind: 'error', message: e instanceof Error ? e.message : 'Could not turn off notifications.' };
  }
}

export function readEnv(): PushEnv {
  const ua = navigator.userAgent;
  const isIOS = /iPad|iPhone|iPod/.test(ua) || (ua.includes('Mac') && navigator.maxTouchPoints > 1);
  return {
    hasServiceWorker: 'serviceWorker' in navigator,
    hasPushManager: typeof window !== 'undefined' && 'PushManager' in window,
    hasNotification: typeof Notification !== 'undefined',
    isIOS,
    isStandalone:
      window.matchMedia?.('(display-mode: standalone)').matches === true ||
      (navigator as unknown as { standalone?: boolean }).standalone === true,
  };
}

function b64urlToBytes(value: string): Uint8Array {
  const padded = value.replace(/-/g, '+').replace(/_/g, '/').padEnd(Math.ceil(value.length / 4) * 4, '=');
  const raw = atob(padded);
  return Uint8Array.from(raw, (c) => c.charCodeAt(0));
}

/** Real dependencies against the service worker and the bridge at `baseUrl`. */
export function browserDeps(baseUrl: string): PushDeps {
  async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
    const headers = mergeBridgeHeaders({ ...ngrokRequestHeaders(baseUrl) });
    if (body) headers['Content-Type'] = 'application/json';
    const res = await fetch(`${baseUrl}${path}`, { method, headers, body: body ? JSON.stringify(body) : undefined });
    if (!res.ok) throw new Error(`${method} ${path} -> ${res.status}`);
    return (await res.json()) as T;
  }
  const registration = () => navigator.serviceWorker.ready;
  return {
    permission: () => Notification.permission,
    requestPermission: () => Notification.requestPermission(),
    isSubscribed: async () => (await (await registration()).pushManager.getSubscription()) !== null,
    subscribe: async (key) => {
      const reg = await registration();
      const sub = await reg.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: b64urlToBytes(key) as BufferSource });
      const json = sub.toJSON();
      return { endpoint: sub.endpoint, keys: { p256dh: json.keys?.p256dh ?? '', auth: json.keys?.auth ?? '' } };
    },
    unsubscribe: async () => {
      const sub = await (await registration()).pushManager.getSubscription();
      await sub?.unsubscribe();
    },
    fetchKey: async () => (await call<{ public_key: string }>('GET', '/push/key')).public_key,
    sendSubscription: async (sub) => {
      await call('POST', '/push/subscribe', sub);
    },
    removeSubscription: async () => {
      await call('DELETE', '/push/subscribe');
    },
  };
}

/** Tell the PC whether this PWA is on screen, so it does not notify a phone that is looking. */
export async function reportPresence(baseUrl: string, visible: boolean): Promise<void> {
  const headers = mergeBridgeHeaders({ ...ngrokRequestHeaders(baseUrl) });
  headers['Content-Type'] = 'application/json';
  try {
    await fetch(`${baseUrl}/push/presence`, { method: 'POST', headers, body: JSON.stringify({ visible }), keepalive: true });
  } catch {
    /* best effort */
  }
}
