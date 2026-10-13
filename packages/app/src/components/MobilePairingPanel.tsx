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
  isLoopbackOrigin,
} from '../lib/public-bridge-url';
import { watchTunnelDevInfo } from '../lib/load-tunnel-dev-info';
import { originNotice } from '../lib/origin-notice';
import { NumberControl, TextControl } from './settings/controls';
import { Btn, Note } from './settings/settings-ui';

/** Fired on window when paired devices change outside this panel (Settings > Phone > Revoke all). */
export const DEVICES_CHANGED_EVENT = 'pm-devices-changed';

interface Props {
  publicPwaUrl: string;
  devServerPort: number;
  onPublicPwaUrlChange: (value: string) => void;
  onDevServerPortChange: (port: number) => void;
}

/** A typed address becomes its origin when it is saved ("pm.example.com" -> "https://pm.example.com"). */
function normalizePwaUrlText(text: string): string {
  if (!text.trim()) return '';
  return publicOriginFromBridgeUrl(normalizePublicBridgeUrl(text));
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
  const prevPwaOriginRef = useRef<string | null>(null);

  const refreshDevices = useCallback(async () => {
    const list = await tauri.listPairedMobileDevices();
    setDevices(list);
  }, []);

  useEffect(() => {
    void refreshDevices();
    // "Revoke all devices" (a separate row) announces itself so this list does not go stale.
    const onDevicesChanged = () => void refreshDevices();
    window.addEventListener(DEVICES_CHANGED_EVENT, onDevicesChanged);
    const stopWatching = watchTunnelDevInfo(setDevInfo);
    return () => {
      window.removeEventListener(DEVICES_CHANGED_EVENT, onDevicesChanged);
      stopWatching();
    };
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
    const ready = Boolean(publicPwaUrl.trim()) || Boolean(devInfo?.tunnelUrl);
    if (!ready) return;
    const prev = prevPwaOriginRef.current;
    prevPwaOriginRef.current = pwaOrigin;
    if (!qrReadyRef.current) {
      qrReadyRef.current = true;
      void refreshPairing();
      return;
    }
    if (prev !== pwaOrigin && !isLoopbackOrigin(pwaOrigin)) {
      void refreshPairing();
    }
  }, [devInfo, publicPwaUrl, refreshPairing, pwaOrigin]);

  const revoke = async (deviceId: string) => {
    setErr(null);
    try {
      await tauri.revokePairedMobileDevice(deviceId);
      await refreshDevices();
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    }
  };

  const expiresLabel = session
    ? new Date(session.expires_at * 1000).toLocaleTimeString()
    : '—';

  return (
    <div className="st-row is-block" data-testid="pairing-panel">
      <div className="st-label">
        <span>Pair a phone</span>
        <small>Scan the QR code with your phone camera. It opens a link and signs you in.</small>
      </div>
      <div className="st-body space-y-3">
        <div>
          <label htmlFor="pairing-public-url" className="block text-[11px] text-pm-muted mb-1">
            Public PWA URL
          </label>
          <TextControl
            id="pairing-public-url"
            aria-label="Public PWA URL"
            type="url"
            mono
            value={publicPwaUrl}
            placeholder="Leave empty to use dev tunnel URL"
            normalize={normalizePwaUrlText}
            onCommit={onPublicPwaUrlChange}
          />
          {tunnelOrigin && publicPwaUrl.trim() && publicPwaUrl.replace(/\/+$/, '') !== tunnelOrigin && (
            <p className="text-[10px] text-pm-muted mb-1">
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
          <label htmlFor="pairing-dev-port" className="block text-[10px] text-pm-muted mb-1 mt-2">
            Local dev server port
          </label>
          <NumberControl
            id="pairing-dev-port"
            aria-label="Local dev server port"
            min={1024}
            max={65535}
            value={devServerPort}
            onCommit={onDevServerPortChange}
          />
          <p className="text-[10px] text-pm-muted mt-1">
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
            {devInfo?.tunnelUrl && !publicPwaUrl.trim() && (
              <span> · from dev tunnel (set a custom URL above to override)</span>
            )}
          </p>
          <p className="text-[10px] text-pm-muted mt-1">
            Dev CLI: <code className="font-mono">PUPPET_MASTER_PUBLIC_URL=https://your.domain</code>
          </p>
        </div>

        {(() => {
          const notice = originNotice(pwaOrigin);
          if (!notice) return null;
          return (
            <div className="rounded border border-pm-border p-2 text-xs" data-testid="pairing-origin">
              <span className="text-pm-muted">Your phone will connect to </span>
              <strong className="font-mono break-all">{notice.host}</strong>
              {notice.warning && <p className="mt-1 text-pm-muted">{notice.warning}</p>}
            </div>
          );
        })()}
        <div className="flex flex-col sm:flex-row gap-4 items-start">
          <div className="rounded-lg border border-pm-border bg-pm-bg p-2 shrink-0">
            <canvas ref={canvasRef} className="block" aria-label="Mobile pairing QR code" />
          </div>
          <div className="flex-1 min-w-0 space-y-2 text-xs">
            <div>
              <div className="text-[10px] text-pm-muted">Pairing code</div>
              <code className="text-sm font-mono">{session?.pairing_code ?? '…'}</code>
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

        {err && <Note role="alert">{err}</Note>}

        <div>
          <div className="text-[10px] text-pm-muted mb-1">Paired devices</div>
          {devices.length === 0 ? (
            <p className="text-[10px] text-pm-muted">No phones paired yet.</p>
          ) : (
            <ul className="space-y-1">
              {devices.map((d) => (
                <li key={d.id} className="flex items-center gap-2 text-xs">
                  <span className="flex-1 truncate">{d.name}</span>
                  <span className="text-[10px] text-pm-muted font-mono truncate max-w-[8rem]">{d.id.slice(0, 8)}…</span>
                  <Btn small onClick={() => void revoke(d.id)}>
                    Revoke
                  </Btn>
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>
    </div>
  );
}
