export const BRIDGE_PROXY_PREFIX = '/bridge';
export const LS_BRIDGE_URL = 'pm-bridge-url';
const DEFAULT_LOCAL_BRIDGE = 'http://127.0.0.1:17321';

export type BridgeLocation = Pick<Location, 'origin' | 'protocol' | 'hostname'>;

/** True when the PWA is served over HTTPS or a non-loopback host (e.g. Cloudflare tunnel). */
export function shouldUseSameOriginBridgeProxy(location: BridgeLocation): boolean {
  if (location.protocol === 'https:') return true;
  return location.hostname !== '127.0.0.1' && location.hostname !== 'localhost';
}

function sameOriginBridgeProxyUrl(location: BridgeLocation): string {
  return `${location.origin}${BRIDGE_PROXY_PREFIX}`;
}

function urlOrigin(url: string): string | null {
  try {
    return new URL(url).origin;
  } catch {
    return null;
  }
}

/** Ensure a bridge base URL ends with `/bridge`. */
export function normalizeBridgeBaseUrl(url: string): string {
  const trimmed = url.trim().replace(/\/+$/, '');
  if (!trimmed) return trimmed;
  if (trimmed.endsWith(BRIDGE_PROXY_PREFIX)) return trimmed;
  try {
    const parsed = new URL(trimmed.includes('://') ? trimmed : `https://${trimmed}`);
    return `${parsed.origin}${BRIDGE_PROXY_PREFIX}`;
  } catch {
    return `${trimmed}${BRIDGE_PROXY_PREFIX}`;
  }
}

/**
 * Pick the bridge base URL for the mobile PWA.
 * - On a tunneled/public origin, always use same-origin `/bridge` (ignores stale saved hosts).
 * - Saved URL wins on localhost dev.
 */
export function resolveBridgeBaseUrl(
  storedUrl: string | null,
  location: BridgeLocation,
): string {
  if (shouldUseSameOriginBridgeProxy(location)) {
    const stored = storedUrl?.replace(/\/$/, '') ?? null;
    const storedOrigin = stored ? urlOrigin(stored) : null;
    if (!stored || storedOrigin !== location.origin) {
      return sameOriginBridgeProxyUrl(location);
    }
    return normalizeBridgeBaseUrl(stored);
  }

  const stored = storedUrl?.replace(/\/$/, '') ?? null;
  if (stored) return normalizeBridgeBaseUrl(stored);
  return DEFAULT_LOCAL_BRIDGE;
}
