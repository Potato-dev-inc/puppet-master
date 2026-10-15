// Connection setup screen of the mobile PWA: pair via invite code / legacy QR JSON, or enter a bridge URL by hand.
import { useEffect, useState } from 'react';
import { LS_BRIDGE_URL, resolveBridgeBaseUrl, shouldUseSameOriginBridgeProxy } from '../../lib/bridge-url';
import { ngrokRequestHeaders } from '../../lib/bridge-ngrok';
import { mergeBridgeHeaders } from '../../lib/mobile-pairing-auth';
import {
  clearMobilePairingCredentials,
  loadMobilePairingCredentials,
  pairFromInviteCode,
  pairFromQrJson,
} from '../../lib/mobile-pairing';

interface DevInfo {
  tunnelUrl?: string | null;
  ngrokUrl?: string | null;
  bridgeProxyUrl?: string;
  tunnelProvider?: string | null;
}

export function SetupScreen({ onConnect }: { onConnect: (url: string) => void }) {
  const [mode, setMode] = useState<'pair' | 'manual'>('pair');
  const [url, setUrl] = useState(() => resolveBridgeBaseUrl(localStorage.getItem(LS_BRIDGE_URL), window.location));
  const [testing, setTesting] = useState(false);
  const [pairing, setPairing] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [devInfo, setDevInfo] = useState<DevInfo | null>(null);
  const [qrJson, setQrJson] = useState('');
  const [manualCode, setManualCode] = useState('');
  const [deviceName, setDeviceName] = useState(() => {
    const ua = navigator.userAgent;
    if (/iPhone/i.test(ua)) return 'iPhone';
    if (/iPad/i.test(ua)) return 'iPad';
    if (/Android/i.test(ua)) return 'Android';
    return 'Mobile device';
  });
  const existingCreds = loadMobilePairingCredentials();

  useEffect(() => {
    fetch('/__puppet_master_dev__.json', { cache: 'no-store' })
      .then((res) => (res.ok ? res.json() as Promise<DevInfo> : null))
      .then((info) => {
        const publicUrl = info?.tunnelUrl ?? info?.ngrokUrl;
        if (publicUrl) setDevInfo({ ...info, tunnelUrl: publicUrl });
      })
      .catch(() => {});
  }, []);

  const handleConnect = async (targetUrl?: string) => {
    setTesting(true);
    setErr(null);
    const base = (targetUrl ?? url).replace(/\/$/, '');
    try {
      const res = await fetch(`${base}/health`, {
        signal: AbortSignal.timeout(4000),
        headers: mergeBridgeHeaders(ngrokRequestHeaders(base)),
      });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      localStorage.setItem(LS_BRIDGE_URL, base);
      onConnect(base);
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setTesting(false);
    }
  };

  const handlePairFromInvite = async (pairingCode: string) => {
    setPairing(true);
    setErr(null);
    try {
      const creds = await pairFromInviteCode(
        pairingCode,
        deviceName.trim() || 'Mobile device',
        window.location,
        localStorage.getItem(LS_BRIDGE_URL),
      );
      setUrl(creds.bridgeUrl);
      await handleConnect(creds.bridgeUrl);
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setPairing(false);
    }
  };

  const handlePair = async (payload?: string) => {
    const json = (payload ?? qrJson).trim();
    if (!json) return;
    setPairing(true);
    setErr(null);
    try {
      const creds = await pairFromQrJson(json, deviceName.trim() || 'Mobile device');
      setUrl(creds.bridgeUrl);
      setQrJson(json);
      await handleConnect(creds.bridgeUrl);
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setPairing(false);
    }
  };

  const useTunnelBridge = () => {
    if (!devInfo?.bridgeProxyUrl) return;
    setUrl(devInfo.bridgeProxyUrl);
  };

  return (
    <div className="flex flex-col items-center justify-center h-full p-6 gap-4 text-sm">
      <img src="/app-icon.svg" alt="Puppet Master" className="w-16 h-16 rounded-xl" />
      <h1 className="text-lg font-semibold">Puppet Master</h1>
      <p className="text-pm-muted text-center text-xs max-w-xs">
        {shouldUseSameOriginBridgeProxy(window.location)
          ? 'On desktop: Settings → Mobile pairing. Scan the QR with your phone camera — it opens this app and signs you in.'
          : 'Pair with the desktop QR code first. Direct localhost works without pairing on this machine.'}
      </p>
      {devInfo?.tunnelUrl && !shouldUseSameOriginBridgeProxy(window.location) && (
        <div className="w-full max-w-xs rounded border border-pm-border bg-pm-raised/40 p-3 text-xs space-y-2">
          <div className="text-pm-muted uppercase tracking-wide text-[10px]">Phone / other network</div>
          <a
            href={devInfo.tunnelUrl}
            className="block text-pm-accent break-all hover:underline"
            target="_blank"
            rel="noreferrer"
          >
            {devInfo.tunnelUrl}
          </a>
          {devInfo.tunnelProvider && (
            <div className="text-[10px] text-pm-muted">via {devInfo.tunnelProvider}</div>
          )}
          {devInfo.bridgeProxyUrl && (
            <button
              type="button"
              onClick={useTunnelBridge}
              className="text-[10px] text-pm-muted hover:text-zinc-200 underline"
            >
              Use tunnel bridge URL for testing on this machine
            </button>
          )}
        </div>
      )}
      <div className="flex gap-2 text-xs">
        <button
          type="button"
          onClick={() => setMode('pair')}
          className={`px-3 py-1 rounded border ${mode === 'pair' ? 'border-pm-accent text-pm-accent bg-pm-accent/10' : 'border-pm-border text-pm-muted'}`}
        >
          Pair
        </button>
        <button
          type="button"
          onClick={() => setMode('manual')}
          className={`px-3 py-1 rounded border ${mode === 'manual' ? 'border-pm-accent text-pm-accent bg-pm-accent/10' : 'border-pm-border text-pm-muted'}`}
        >
          Manual URL
        </button>
      </div>

      {existingCreds && (
        <div className="w-full max-w-xs rounded border border-pm-border bg-pm-raised/40 p-3 text-xs space-y-2">
          <div className="text-pm-muted">Paired as <span className="text-zinc-200">{existingCreds.deviceName}</span></div>
          <div className="flex gap-2">
            <button
              type="button"
              onClick={() => void handleConnect(existingCreds.bridgeUrl)}
              disabled={testing}
              className="flex-1 px-3 py-1.5 rounded border border-pm-accent/50 text-pm-accent"
            >
              Reconnect
            </button>
            <button
              type="button"
              onClick={() => {
                clearMobilePairingCredentials();
                window.location.reload();
              }}
              className="px-3 py-1.5 rounded border border-pm-border text-pm-muted"
            >
              Unpair
            </button>
          </div>
        </div>
      )}

      {mode === 'pair' ? (
        <div className="w-full max-w-xs space-y-2">
          <div className="rounded border border-pm-border bg-pm-raised/30 p-3 text-xs text-pm-muted space-y-1">
            <p>1. Open <strong className="text-zinc-200">Settings → Mobile pairing</strong> on desktop.</p>
            <p>2. Scan the QR with your phone <strong className="text-zinc-200">Camera</strong> app.</p>
            <p>3. Tap the link — Puppet Master opens and connects.</p>
          </div>
          {err && <p className="text-pm-err text-xs">{err}</p>}
          {pairing && (
            <p className="text-xs text-pm-muted text-center">Pairing…</p>
          )}
          <details className="text-xs">
            <summary className="text-pm-muted cursor-pointer select-none">Advanced: paste code or legacy JSON</summary>
            <div className="mt-2 space-y-2">
              <label className="block text-[10px] text-pm-muted uppercase tracking-wide">Device name</label>
              <input
                value={deviceName}
                onChange={(e) => setDeviceName(e.target.value)}
                className="w-full bg-pm-bg border border-pm-border rounded px-3 py-2 text-xs"
              />
              <label className="block text-[10px] text-pm-muted uppercase tracking-wide">Pairing code</label>
              <input
                value={manualCode}
                onChange={(e) => setManualCode(e.target.value.toUpperCase())}
                placeholder="PM8K2X4Q"
                className="w-full bg-pm-bg border border-pm-border rounded px-3 py-2 text-xs font-mono uppercase"
                onKeyDown={(e) => {
                  if (e.key === 'Enter' && manualCode.trim()) void handlePairFromInvite(manualCode.trim());
                }}
              />
              <button
                type="button"
                onClick={() => manualCode.trim() && void handlePairFromInvite(manualCode.trim())}
                disabled={pairing || !manualCode.trim()}
                className="w-full px-4 py-2 rounded border border-pm-border text-pm-muted text-xs disabled:opacity-50"
              >
                Pair with code
              </button>
              <textarea
                value={qrJson}
                onChange={(e) => setQrJson(e.target.value)}
                placeholder='Legacy JSON QR payload'
                className="w-full min-h-[56px] bg-pm-bg border border-pm-border rounded px-3 py-2 text-[10px] font-mono"
              />
              <button
                type="button"
                onClick={() => void handlePair()}
                disabled={pairing || !qrJson.trim()}
                className="w-full px-4 py-2 rounded border border-pm-border text-pm-muted text-xs disabled:opacity-50"
              >
                Pair legacy JSON
              </button>
            </div>
          </details>
        </div>
      ) : (
        <>
      <input
        type="url"
        value={url}
        onChange={(e) => setUrl(e.target.value)}
        placeholder={shouldUseSameOriginBridgeProxy(window.location) ? 'https://your.domain.com/bridge' : 'http://127.0.0.1:17321'}
        className="w-full max-w-xs bg-pm-bg border border-pm-border rounded px-3 py-2 text-xs text-zinc-100 focus:outline-none focus:border-pm-accent"
      />
      {err && <p className="text-pm-err text-xs">{err}</p>}
      <button
        onClick={() => void handleConnect()}
        disabled={testing}
        className="px-4 py-2 rounded bg-pm-accent/20 border border-pm-accent text-pm-accent text-xs disabled:opacity-50"
      >
        {testing ? 'Connecting…' : 'Connect'}
      </button>
        </>
      )}
    </div>
  );
}
