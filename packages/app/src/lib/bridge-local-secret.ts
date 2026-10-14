// The bridge's per-launch local key (S3). The desktop app gets it over Tauri IPC and sends it as
// `X-PM-Local-Secret` on its own HTTP calls to the loopback bridge. The bridge only logs a local
// caller without it for now (`LOCAL_SECRET_MISSING`). The key is never sent anywhere but loopback:
// the phone PWA has no Tauri and never sees it.
import { tauri } from './tauri';

export const LOCAL_SECRET_HEADER = 'X-PM-Local-Secret';

let secret: string | null = null;
let loading: Promise<string | null> | null = null;

/** Fetch the key once per window (null outside the desktop app or on an older app). */
export function loadBridgeLocalSecret(): Promise<string | null> {
  loading ??= tauri.bridgeLocalSecret().then((value) => {
    secret = typeof value === 'string' && /^[0-9a-f]{16,128}$/i.test(value) ? value : null;
    return secret;
  });
  return loading;
}

export function hasBridgeLocalSecret(): boolean {
  return secret !== null;
}

/** For tests. */
export function setBridgeLocalSecretForTest(value: string | null): void {
  secret = value;
  loading = Promise.resolve(value);
}

export function isLoopbackBridgeUrl(baseUrl: string): boolean {
  try {
    const host = new URL(baseUrl).hostname.replace(/^\[|\]$/g, '').toLowerCase();
    return host === 'localhost' || host.endsWith('.localhost') || host === '::1' || /^127\.\d+\.\d+\.\d+$/.test(host);
  } catch {
    return false;
  }
}

/** The header to add to a bridge request: only to a loopback bridge, only when the key is known. */
export function localSecretHeaders(baseUrl: string): Record<string, string> {
  if (!secret || !isLoopbackBridgeUrl(baseUrl)) return {};
  return { [LOCAL_SECRET_HEADER]: secret };
}
