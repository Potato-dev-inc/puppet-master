import { buildPairingInviteUrl, encodePairingQrContent, PairingQrPayloadSchema, type PairingSession } from '@puppet-master/shared';
import { useCallback, useEffect, useRef, useState } from 'react';
import QRCode from 'qrcode';
import { tauri } from '../lib/tauri';
import {
  DEFAULT_DEV_SERVER_PORT,
  localDevPwaUrl,
  normalizePublicBridgeUrl,
  publicOriginFromBridgeUrl,
  resolvePairingBridgeUrl,
  resolvePwaOriginForQr,
  type PublicBridgeDevInfo,
} from '../lib/public-bridge-url';

interface Props {
  publicPwaUrl: string;
  devServerPort: number;
  onPublicPwaUrlChange: (value: string) => void;
  onDevServerPortChange: (port: number) => void;
}

export function MobilePairingPanel({
  publicPwaUrl,
  devServerPort,
  onPublicPwaUrlChange,
  onDevServerPortChange,
}: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [devInfo, setDevInfo] = useState<PublicBridgeDevInfo | null>(null);
  const [session, setSession] = useState<PairingSession | null>(null);
  const [devices, setDevices] = useState<Awaited<ReturnType<typeof tauri.listPairedMobileDevices>>>([]);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const bridgeUrl = resolvePairingBridgeUrl(devInfo, publicPwaUrl, devServerPort);
  const { origin: pwaOrigin, source: qrSource } = resolvePwaOriginForQr(devInfo, publicPwaUrl, devServerPort);
  const localProxyTarget = localDevPwaUrl(devServerPort);
  const tunnelOrigin = devInfo?.tunnelUrl?.replace(/\/+$/, '') ?? null;
  const qrReadyRef = useRef(false);

  const refreshDevices = useCallback(async () => {
    const list = await tauri.listPairedMobileDevices();
    setDevices(list);
  }, []);

  useEffect(() => {
    void refreshDevices();
    fetch('/__puppet_master_dev__.json', { cache: 'no-store' })
      .then((res) => (res.ok ? res.json() as Promise<PublicBridgeDevInfo> : null))
      .then((info) => setDevInfo(info))
      .catch(() => {});
  }, [refreshDevices]);

  const refreshPairing = useCallback(async () => {
    setBusy(true);
    setErr(null);
    try {
      const next = await tauri.createMobilePairingSession(bridgeUrl);
      const payload = PairingQrPayloadSchema.parse(next.qr_payload);
      setSession(next);
      const qrText = encodePairingQrContent(payload, pwaOrigin);
      if (canvasRef.current) {
        await QRCode.toCanvas(canvasRef.current, qrText, {
          width: 220,
          margin: 2,
          color: { dark: '#e8e8e8', light: '#121212' },
        });
      }
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }, [bridgeUrl, pwaOrigin]);

  useEffect(() => {
    if (qrReadyRef.current) return;
    const ready = Boolean(publicPwaUrl.trim()) || devInfo !== null;
    if (!ready) return;
    qrReadyRef.current = true;
    void refreshPairing();
  }, [devInfo, publicPwaUrl, refreshPairing]);

  const revoke = async (deviceId: string) => {
    await tauri.revokePairedMobileDevice(deviceId);
    await refreshDevices();
  };

  const expiresLabel = session
    ? new Date(session.expires_at * 1000).toLocaleTimeString()
    : '—';

  return (
    <div className="border border-pm-border rounded p-3 mb-3 space-y-3">
      <div>
        <h3 className="text-xs font-semibold">Mobile pairing</h3>
        <p className="text-[10px] text-pm-muted mt-1">
          Scan with your phone camera — it opens a link and signs you in automatically.
        </p>
      </div>

      <div>
        <label className="block text-[10px] text-pm-muted uppercase tracking-wide mb-1">
          Public PWA URL
        </label>
        <input
          type="url"
          value={publicPwaUrl}
          onChange={(e) => onPublicPwaUrlChange(e.target.value)}
          onBlur={() => {
            if (!publicPwaUrl.trim()) return;
            const normalized = normalizePublicBridgeUrl(publicPwaUrl);
            onPublicPwaUrlChange(publicOriginFromBridgeUrl(normalized));
          }}
          placeholder="Leave empty to use dev tunnel URL"
          className="w-full text-xs bg-pm-bg border border-pm-border rounded px-2 py-1.5 font-mono mb-1"
        />
        {tunnelOrigin && publicPwaUrl.trim() && publicPwaUrl.replace(/\/+$/, '') !== tunnelOrigin && (
          <p className="text-[10px] text-pm-amber-400 mb-1">
            Custom URL must resolve on your phone. Dev tunnel:{' '}
            <button
              type="button"
              className="underline font-mono"
              onClick={() => onPublicPwaUrlChange(tunnelOrigin)}
            >
              {tunnelOrigin}
            </button>
          </p>
        )}
        {tunnelOrigin && !publicPwaUrl.trim() && (
          <p className="text-[10px] text-pm-muted mb-1">
            Using dev tunnel for QR: <code className="font-mono">{tunnelOrigin}</code>
          </p>
        )}
        <label className="block text-[10px] text-pm-muted uppercase tracking-wide mb-1 mt-2">
          Local dev server port
        </label>
        <input
          type="number"
          min={1024}
          max={65535}
          value={devServerPort}
          onChange={(e) => {
            const value = Number(e.target.value);
            if (!Number.isFinite(value)) return;
            onDevServerPortChange(Math.min(65535, Math.max(1024, Math.round(value))));
          }}
          className="w-full text-xs bg-pm-bg border border-pm-border rounded px-2 py-1.5 font-mono mb-1"
        />
        <p className="text-[10px] text-pm-muted">
          Proxy your public URL to <code className="font-mono">{localProxyTarget}</code> (Vite default{' '}
          <code className="font-mono">{DEFAULT_DEV_SERVER_PORT}</code>).
        </p>
        <p className="text-[10px] text-pm-muted mt-1">
          Phone opens ({qrSource}):{' '}
          <code className="font-mono break-all">
            {session ? buildPairingInviteUrl(pwaOrigin, session.pairing_code) : '…'}
          </code>
        </p>
        <p className="text-[10px] text-pm-muted mt-1">
          Bridge: <code className="font-mono">{bridgeUrl}</code>
          {devInfo?.bridgeProxyUrl && !publicPwaUrl.trim() && (
            <span> · from dev tunnel (set a custom URL above to override)</span>
          )}
        </p>
        <p className="text-[10px] text-pm-muted mt-1">
          Dev CLI: <code className="font-mono">PUPPET_MASTER_PUBLIC_URL=https://your.domain</code>
        </p>
      </div>

      <div className="flex flex-col sm:flex-row gap-4 items-start">
        <div className="rounded-lg border border-pm-border bg-pm-bg p-2 shrink-0">
          <canvas ref={canvasRef} className="block" aria-label="Mobile pairing QR code" />
        </div>
        <div className="flex-1 min-w-0 space-y-2 text-xs">
          <div>
            <div className="text-[10px] text-pm-muted uppercase tracking-wide">Pairing code</div>
            <code className="text-sm font-mono text-pm-accent">{session?.pairing_code ?? '…'}</code>
          </div>
          <div className="text-[10px] text-pm-muted">Expires {expiresLabel}</div>
          <button
            type="button"
            disabled={busy}
            onClick={() => {
              qrReadyRef.current = true;
              void refreshPairing();
            }}
            className="px-2 py-1 text-xs rounded border border-pm-border hover:bg-pm-border/40 disabled:opacity-50"
          >
            {busy ? 'Refreshing…' : 'Refresh QR'}
          </button>
        </div>
      </div>

      {err && <p className="text-xs text-pm-err">{err}</p>}

      <div>
        <div className="text-[10px] text-pm-muted uppercase tracking-wide mb-1">Paired devices</div>
        {devices.length === 0 ? (
          <p className="text-[10px] text-pm-muted">No phones paired yet.</p>
        ) : (
          <ul className="space-y-1">
            {devices.map((d) => (
              <li key={d.id} className="flex items-center gap-2 text-xs">
                <span className="flex-1 truncate">{d.name}</span>
                <span className="text-[10px] text-pm-muted font-mono truncate max-w-[8rem]">{d.id.slice(0, 8)}…</span>
                <button
                  type="button"
                  onClick={() => void revoke(d.id)}
                  className="text-[10px] text-pm-err hover:underline"
                >
                  Revoke
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
