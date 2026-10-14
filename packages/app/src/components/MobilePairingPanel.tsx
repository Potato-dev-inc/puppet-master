import {
  buildPairingInviteUrl,
  DEVICE_CAPS,
  encodePairingQrContent,
  PairingQrPayloadSchema,
  type DeviceCap,
  type PairingSession,
} from '@puppet-master/shared';
import { useCallback, useEffect, useId, useRef, useState } from 'react';
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
import { Note } from './settings/settings-ui';

/** Fired on window when paired devices change (a phone paired, Revoke, Revoke all, abilities). */
export const DEVICES_CHANGED_EVENT = 'pm-devices-changed';

/** What a newly paired phone may do. Full control is today's behaviour and the default. */
export const PAIRING_CHOICES: { id: 'full' | 'watch_answer'; label: string; hint: string; caps: readonly DeviceCap[] }[] = [
  {
    id: 'full',
    label: 'Full control',
    hint: 'Watch, answer, use the terminal and start tasks, like sitting at this PC.',
    caps: DEVICE_CAPS,
  },
  {
    id: 'watch_answer',
    label: 'Watch and answer only',
    hint: "See your agents and answer their questions. It can't type or start anything.",
    caps: ['watch', 'answer'],
  },
];

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
  const [choice, setChoice] = useState<(typeof PAIRING_CHOICES)[number]['id']>('full');
  const choiceGroup = useId();
  const [, setPairedCount] = useState<number | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const bridgeUrl = resolvePairingBridgeUrl(devInfo, publicPwaUrl, devServerPort);
  const { origin: pwaOrigin, source: qrSource } = resolvePwaOriginForQr(devInfo, publicPwaUrl, devServerPort);
  const localProxyTarget = localDevPwaUrl(devServerPort);
  const tunnelOrigin = devInfo?.tunnelUrl?.replace(/\/+$/, '') ?? null;
  const qrReadyRef = useRef(false);

  // A phone that pairs while this panel is open shows up in the Devices list below.
  const refreshDevices = useCallback(async () => {
    const list = await tauri.listPairedMobileDevices();
    setPairedCount((prev) => {
      if (prev !== null && list.length !== prev) window.dispatchEvent(new Event(DEVICES_CHANGED_EVENT));
      return list.length;
    });
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
      const caps = PAIRING_CHOICES.find((c) => c.id === choice)?.caps ?? DEVICE_CAPS;
      const next = await tauri.createMobilePairingSession(bridgeUrl, caps);
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
  }, [bridgeUrl, pwaOrigin, choice]);

  // A different choice needs a new code: the choice is bound to the code on the PC.
  const prevChoiceRef = useRef(choice);
  useEffect(() => {
    if (prevChoiceRef.current === choice) return;
    prevChoiceRef.current = choice;
    if (qrReadyRef.current) void refreshPairing();
  }, [choice, refreshPairing]);

  // While the code is up, notice the phone that redeems it (the Devices list refreshes).
  useEffect(() => {
    if (!session) return;
    const timer = window.setInterval(() => void refreshDevices().catch(() => undefined), 3000);
    return () => window.clearInterval(timer);
  }, [session, refreshDevices]);

  // Opening this tab never makes a code: a code is a record in the audit log and a live way in, so it
  // is made only by the "Pair a phone" button. If the address the phone would use changes while a code
  // is up, that code points at the old address: drop it and let the person make a new one.
  const shownOriginRef = useRef<string | null>(null);
  useEffect(() => {
    const prev = shownOriginRef.current;
    shownOriginRef.current = pwaOrigin;
    if (prev !== null && prev !== pwaOrigin && qrReadyRef.current && !isLoopbackOrigin(pwaOrigin)) {
      qrReadyRef.current = false;
      setSession(null);
    }
  }, [pwaOrigin]);

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
              {session ? buildPairingInviteUrl(pwaOrigin, session.pairing_code) : 'Shown once you make a code'}
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

        <fieldset className="st-choices" data-testid="pairing-choice">
          <legend>What can this phone do?</legend>
          {PAIRING_CHOICES.map((option) => (
            <label key={option.id} className="st-choice" data-selected={choice === option.id ? 'true' : undefined}>
              <input
                type="radio"
                name={choiceGroup}
                value={option.id}
                checked={choice === option.id}
                onChange={() => setChoice(option.id)}
              />
              <span className="st-label">
                <span>{option.label}</span>
                <small>{option.hint}</small>
              </span>
            </label>
          ))}
          <p className="st-note">You can change this later under Devices. A new choice makes a new code.</p>
        </fieldset>

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
          <div className="rounded-lg border border-pm-border bg-pm-bg p-2 shrink-0" hidden={!session}>
            <canvas ref={canvasRef} className="block" aria-label="Mobile pairing QR code" />
          </div>
          {session ? (
            <div className="flex-1 min-w-0 space-y-2 text-xs" data-testid="pairing-code">
              <div>
                <div className="text-[10px] text-pm-muted">Pairing code</div>
                <code className="text-sm font-mono">{session.pairing_code}</code>
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
          ) : (
            <div className="flex-1 min-w-0 space-y-2 text-xs">
              <button
                type="button"
                data-testid="pair-a-phone"
                disabled={busy}
                onClick={() => {
                  qrReadyRef.current = true;
                  void refreshPairing();
                }}
                className="px-2 py-1 text-xs rounded border border-pm-border hover:bg-pm-border/40 disabled:opacity-50"
              >
                {busy ? 'Making a code…' : 'Pair a phone'}
              </button>
              <div className="text-[10px] text-pm-muted">Makes a one-time code. It expires after a few minutes.</div>
            </div>
          )}
        </div>

        {err && <Note role="alert">{err}</Note>}

      </div>
    </div>
  );
}
