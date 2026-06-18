export const BRIDGE_PROXY_PREFIX = '/bridge';
export const LS_BRIDGE_URL = 'pm-bridge-url';
const DEFAULT_LOCAL_BRIDGE = 'http://127.0.0.1:17321';

export type BridgeLocation = Pick<Location, 'origin' | 'protocol' | 'hostname'>;

/** True when the PWA is served over HTTPS or a non-loopback host (e.g. Cloudflare tunnel). */
export function shouldUseSameOriginBridgeProxy(location: BridgeLocation): boolean {
  if (location.protocol === 'https:') return true;
  return location.hostname !== '127.0.0.1' && location.hostname !== 'localhost';
}

function isLocalBridgeUrl(url: string): boolean {
  try {
    const { hostname } = new URL(url);
    return hostname === '127.0.0.1' || hostname === 'localhost';
  } catch {
    return false;
  }
}

function sameOriginBridgeProxyUrl(location: BridgeLocation): string {
  return `${location.origin}${BRIDGE_PROXY_PREFIX}`;
}

/**
 * Pick the bridge base URL for the mobile PWA.
 * - Saved URL wins unless it points at localhost while we're on a remote/tunneled origin.
 * - Remote/tunneled origins use the Vite `/bridge` proxy (same origin, no mixed content).
 */
export function resolveBridgeBaseUrl(
  storedUrl: string | null,
  location: BridgeLocation,
): string {
  const stored = storedUrl?.replace(/\/$/, '') ?? null;

  if (stored && isLocalBridgeUrl(stored) && shouldUseSameOriginBridgeProxy(location)) {
    return sameOriginBridgeProxyUrl(location);
  }
  if (stored) return stored;
  if (shouldUseSameOriginBridgeProxy(location)) {
    return sameOriginBridgeProxyUrl(location);
  }
  return DEFAULT_LOCAL_BRIDGE;
}
