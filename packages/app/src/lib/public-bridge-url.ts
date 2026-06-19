export const LS_PUBLIC_BRIDGE_URL = 'pm-public-bridge-url';
export const DEFAULT_DEV_SERVER_PORT = 1420;

/** Normalize user input to a bridge base URL ending in `/bridge`. */
export function normalizePublicBridgeUrl(input: string): string {
  const trimmed = input.trim().replace(/\/+$/, '');
  if (!trimmed) return '';

  try {
    const url = new URL(trimmed.includes('://') ? trimmed : `https://${trimmed}`);
    if (url.pathname === '/bridge' || url.pathname.endsWith('/bridge')) {
      return `${url.origin}/bridge`;
    }
    return `${url.origin}/bridge`;
  } catch {
    return trimmed.endsWith('/bridge') ? trimmed : `${trimmed}/bridge`;
  }
}

export function readStoredPublicBridgeUrl(): string | null {
  try {
    return localStorage.getItem(LS_PUBLIC_BRIDGE_URL);
  } catch {
    return null;
  }
}

export function saveStoredPublicBridgeUrl(input: string): void {
  const normalized = normalizePublicBridgeUrl(input);
  if (!normalized) {
    localStorage.removeItem(LS_PUBLIC_BRIDGE_URL);
    return;
  }
  localStorage.setItem(LS_PUBLIC_BRIDGE_URL, normalized);
}

export interface PublicBridgeDevInfo {
  bridgeProxyUrl?: string | null;
  tunnelUrl?: string | null;
  localUrl?: string | null;
}

export function localDevPwaUrl(port = DEFAULT_DEV_SERVER_PORT): string {
  return `http://127.0.0.1:${port}`;
}

export function localDevBridgeUrl(port = DEFAULT_DEV_SERVER_PORT): string {
  return `${localDevPwaUrl(port)}/bridge`;
}

/** Desktop pairing URL: saved custom URL wins, then dev tunnel, then local Vite port. */
export function resolvePairingBridgeUrl(
  devInfo: PublicBridgeDevInfo | null,
  customUrl: string | null,
  devServerPort = DEFAULT_DEV_SERVER_PORT,
): string {
  if (customUrl?.trim()) return normalizePublicBridgeUrl(customUrl);
  if (devInfo?.bridgeProxyUrl) return devInfo.bridgeProxyUrl.replace(/\/$/, '');
  if (devInfo?.tunnelUrl) return `${devInfo.tunnelUrl.replace(/\/$/, '')}/bridge`;
  if (devInfo?.localUrl) {
    try {
      const origin = new URL(devInfo.localUrl).origin;
      return `${origin}/bridge`;
    } catch {
      /* fall through */
    }
  }
  return localDevBridgeUrl(devServerPort);
}

/** Strip `/bridge` suffix to show the PWA origin in UI. */
export function publicOriginFromBridgeUrl(bridgeUrl: string): string {
  return bridgeUrl.replace(/\/bridge\/?$/, '');
}

export function parseDevServerPort(value: unknown): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) return DEFAULT_DEV_SERVER_PORT;
  const rounded = Math.round(value);
  return Math.min(65535, Math.max(1024, rounded));
}

/** PWA origin encoded in the pairing QR (tunnel wins when no custom URL). */
export function resolvePwaOriginForQr(
  devInfo: PublicBridgeDevInfo | null,
  customPublicUrl: string | null,
  devServerPort = DEFAULT_DEV_SERVER_PORT,
): { origin: string; source: 'custom' | 'tunnel' | 'local' } {
  const custom = customPublicUrl?.trim();
  if (custom) {
    return {
      origin: publicOriginFromBridgeUrl(normalizePublicBridgeUrl(custom)),
      source: 'custom',
    };
  }
  if (devInfo?.tunnelUrl) {
    return { origin: devInfo.tunnelUrl.replace(/\/+$/, ''), source: 'tunnel' };
  }
  if (devInfo?.bridgeProxyUrl) {
    return {
      origin: publicOriginFromBridgeUrl(devInfo.bridgeProxyUrl),
      source: 'tunnel',
    };
  }
  return { origin: localDevPwaUrl(devServerPort), source: 'local' };
}
