import { tauri } from './tauri';
import { isLoopbackOrigin, type PublicBridgeDevInfo } from './public-bridge-url';

function mapTunnelInfo(info: NonNullable<Awaited<ReturnType<typeof tauri.getMobileTunnelInfo>>>): PublicBridgeDevInfo {
  return {
    bridgeProxyUrl: info.bridgeProxyUrl,
    tunnelUrl: info.tunnelUrl ?? null,
    localUrl: info.localUrl,
  };
}

function hasPublicTunnel(info: PublicBridgeDevInfo | null): info is PublicBridgeDevInfo {
  if (!info?.tunnelUrl) return false;
  return !isLoopbackOrigin(info.tunnelUrl);
}

/** Production tunnel info from the desktop shell; null when no public URL is active. */
export async function loadDesktopTunnelInfo(): Promise<PublicBridgeDevInfo | null> {
  const info = await tauri.getMobileTunnelInfo();
  if (!info) return null;
  const mapped = mapTunnelInfo(info);
  return hasPublicTunnel(mapped) ? mapped : null;
}

/** Dev-only JSON served by Vite middleware during `tauri dev`. */
export async function loadViteDevInfo(): Promise<PublicBridgeDevInfo | null> {
  try {
    const res = await fetch('/__puppet_master_dev__.json', { cache: 'no-store' });
    if (!res.ok) return null;
    return (await res.json()) as PublicBridgeDevInfo;
  } catch {
    return null;
  }
}

/**
 * Tunnel / dev URLs for mobile pairing.
 * Dev: Vite middleware (cloudflared quick tunnel). Production: embedded Rust tunnel.
 */
export async function loadTunnelDevInfo(): Promise<PublicBridgeDevInfo | null> {
  const [vite, desktop] = await Promise.all([loadViteDevInfo(), loadDesktopTunnelInfo()]);
  if (vite?.tunnelUrl) return vite;
  if (desktop) return desktop;
  return vite;
}

/** Poll until cloudflared publishes a public URL (startup is async). */
export function watchTunnelDevInfo(
  onUpdate: (info: PublicBridgeDevInfo | null) => void,
  options: { intervalMs?: number; timeoutMs?: number } = {},
): () => void {
  const intervalMs = options.intervalMs ?? 1500;
  const timeoutMs = options.timeoutMs ?? 60_000;
  let stopped = false;

  const poll = async () => {
    if (stopped) return;
    const info = await loadTunnelDevInfo();
    onUpdate(info);
    if (info?.tunnelUrl) return;
    if (Date.now() - startedAt >= timeoutMs) return;
    timer = window.setTimeout(() => void poll(), intervalMs);
  };

  const startedAt = Date.now();
  let timer = window.setTimeout(() => void poll(), 0);

  return () => {
    stopped = true;
    window.clearTimeout(timer);
  };
}
