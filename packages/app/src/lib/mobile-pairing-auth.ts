import { loadMobilePairingCredentials } from './mobile-pairing';

/** Bearer token for proxied remote PWA bridge access. */
export function mobilePairingAuthHeaders(): Record<string, string> {
  const creds = loadMobilePairingCredentials();
  if (!creds?.deviceToken) return {};
  return { Authorization: `Bearer ${creds.deviceToken}` };
}

export function mergeBridgeHeaders(extra: Record<string, string>): Record<string, string> {
  return { ...extra, ...mobilePairingAuthHeaders() };
}
