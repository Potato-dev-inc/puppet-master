import { readBridgePort, readLocalSecretHeaders } from '@puppet-master/shared/bridge-port';

/** Where the desktop app serves its dev/production tunnel info (Vite in dev, PWA server otherwise). */
export const PWA_INFO_URL = 'http://127.0.0.1:1420/__puppet_master_dev__.json';

export interface TunnelInfo {
  tunnelUrl?: string | null;
  bridgeProxyUrl?: string | null;
}

export interface PairingSessionResponse {
  pairing_code: string;
  expires_at: number;
  bridge_url: string;
}

function isLoopbackHost(origin: string): boolean {
  try {
    const host = new URL(origin).hostname;
    return host === 'localhost' || host === '127.0.0.1' || host === '[::1]';
  } catch {
    return false;
  }
}

/** Same normalization as the desktop UI: any URL becomes its origin plus `/bridge`. */
export function bridgeUrlForPublicUrl(publicUrl: string): string {
  const trimmed = publicUrl.trim().replace(/\/+$/, '');
  const url = new URL(trimmed.includes('://') ? trimmed : `https://${trimmed}`);
  return `${url.origin}/bridge`;
}

/** The PWA origin a phone opens: the public URL with any `/bridge` suffix removed. */
export function publicOriginForPublicUrl(publicUrl: string): string {
  return bridgeUrlForPublicUrl(publicUrl).replace(/\/bridge$/, '');
}

/**
 * Public origin from the tunnel info, mirroring `resolvePwaOriginForQr`: the tunnel URL wins,
 * then a non-loopback bridge proxy URL. Loopback addresses are not reachable from a phone.
 */
export function publicOriginFromTunnelInfo(info: TunnelInfo | null): string | null {
  if (info?.tunnelUrl && !isLoopbackHost(info.tunnelUrl)) {
    return publicOriginForPublicUrl(info.tunnelUrl);
  }
  if (info?.bridgeProxyUrl && !isLoopbackHost(info.bridgeProxyUrl)) {
    return publicOriginForPublicUrl(info.bridgeProxyUrl);
  }
  return null;
}

export function pairingLink(publicOrigin: string, code: string): string {
  return `${publicOrigin.replace(/\/+$/, '')}/pair/${code}`;
}

async function fetchTunnelInfo(): Promise<TunnelInfo | null> {
  try {
    const res = await fetch(PWA_INFO_URL, { signal: AbortSignal.timeout(3_000) });
    if (!res.ok) return null;
    return (await res.json()) as TunnelInfo;
  } catch {
    return null;
  }
}

/** Env override, then the running desktop app's tunnel info. Throws when neither gives a public URL. */
export async function resolvePublicOrigin(env: NodeJS.ProcessEnv = process.env): Promise<string> {
  const override = env.PUPPET_MASTER_PUBLIC_URL?.trim();
  if (override) return publicOriginForPublicUrl(override);
  const fromApp = publicOriginFromTunnelInfo(await fetchTunnelInfo());
  if (fromApp) return fromApp;
  throw new Error(
    'no public tunnel URL. Start the desktop app with its tunnel enabled, or set PUPPET_MASTER_PUBLIC_URL.',
  );
}

async function postInvite(bridgeUrl: string): Promise<PairingSessionResponse> {
  let host: string;
  let port: number;
  try {
    ({ host, port } = await readBridgePort());
  } catch {
    throw new Error('Puppet Master is not running (no bridge port file). Start the desktop app first.');
  }
  let res: Response;
  try {
    res = await fetch(`http://${host}:${port}/pair/invite`, {
      method: 'POST',
      headers: { ...(await readLocalSecretHeaders()), 'content-type': 'application/json' },
      body: JSON.stringify({ bridge_url: bridgeUrl }),
      signal: AbortSignal.timeout(10_000),
    });
  } catch {
    throw new Error('Puppet Master is not reachable on its bridge. Is the desktop app running?');
  }
  const text = await res.text();
  if (!res.ok) {
    throw new Error(`bridge refused pairing invite (HTTP ${res.status}): ${text.slice(0, 200)}`);
  }
  return JSON.parse(text) as PairingSessionResponse;
}

/** Prints one pairing link on stdout and the expiry on stderr. Returns the process exit code. */
export async function runPairLink(): Promise<number> {
  let publicOrigin: string;
  try {
    publicOrigin = await resolvePublicOrigin();
  } catch (err) {
    console.error(`puppet-master pair-link: ${(err as Error).message}`);
    return 4;
  }
  try {
    const session = await postInvite(bridgeUrlForPublicUrl(publicOrigin));
    console.log(pairingLink(publicOrigin, session.pairing_code));
    console.error(`expires ${new Date(session.expires_at * 1000).toLocaleTimeString()} (single use, 10 minutes)`);
    return 0;
  } catch (err) {
    console.error(`puppet-master pair-link: ${(err as Error).message}`);
    return 4;
  }
}
