import { parsePairingInvitePath } from '@puppet-master/shared';
import { useCallback, useEffect, useRef, useState } from 'react';
import { LS_BRIDGE_URL } from '../lib/bridge-url';
import { pairFromInviteCode } from '../lib/mobile-pairing';

function defaultDeviceName(): string {
  const ua = navigator.userAgent;
  if (/iPhone/i.test(ua)) return 'iPhone';
  if (/iPad/i.test(ua)) return 'iPad';
  if (/Android/i.test(ua)) return 'Android';
  return 'Mobile device';
}

interface Props {
  pairingCode: string;
  onConnected: (bridgeUrl: string) => void;
  onGiveUp: () => void;
}

/** Auto-pair when the user opens an invite link from the desktop QR (/pair/CODE). */
export function PairInviteScreen({ pairingCode, onConnected, onGiveUp }: Props) {
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(true);
  const [stage, setStage] = useState('Connecting to desktop…');
  const onConnectedRef = useRef(onConnected);
  onConnectedRef.current = onConnected;
  const attemptRef = useRef(0);

  const runPairing = useCallback(async (attempt: number) => {
    setBusy(true);
    setErr(null);
    setStage('Connecting to desktop…');
    try {
      const creds = await pairFromInviteCode(
        pairingCode,
        defaultDeviceName(),
        window.location,
        localStorage.getItem(LS_BRIDGE_URL),
        (label) => {
          if (attemptRef.current === attempt) setStage(label);
        },
      );
      if (attemptRef.current !== attempt) return;
      localStorage.setItem(LS_BRIDGE_URL, creds.bridgeUrl);
      onConnectedRef.current(creds.bridgeUrl);
    } catch (e) {
      if (attemptRef.current !== attempt) return;
      setBusy(false);
      const message = e instanceof Error ? e.message : String(e);
      setErr(
        message.includes('no active pairing session')
          ? `${message} Refresh QR on desktop, then scan again immediately.`
          : message.includes('timed out') || message.includes('Timeout')
            ? `${message} Is dev still running? Open /bridge/health on this same URL in the phone browser.`
            : message,
      );
    }
  }, [pairingCode]);

  useEffect(() => {
    attemptRef.current += 1;
    const attempt = attemptRef.current;
    void runPairing(attempt);
  }, [runPairing]);

  return (
    <div className="flex flex-col items-center justify-center h-full p-6 gap-4 text-sm text-center">
      <img src="/app-icon.svg" alt="Puppet Master" className="w-16 h-16 rounded-xl" />
      <h1 className="text-lg font-semibold">Pairing with desktop</h1>
      {busy ? (
        <>
          <p className="text-pm-muted text-xs max-w-xs">{stage}</p>
          <p className="text-pm-muted text-[10px] font-mono">{pairingCode}</p>
          <div className="h-8 w-8 rounded-full border-2 border-pm-accent border-t-transparent animate-spin" />
        </>
      ) : (
        <>
          <p className="text-pm-err text-xs max-w-xs">{err}</p>
          <p className="text-pm-muted text-[10px] max-w-xs">
            Refresh the QR on desktop if it expired, then scan again.
          </p>
          <div className="flex gap-2">
            <button
              type="button"
              onClick={() => void runPairing(++attemptRef.current)}
              className="px-4 py-2 text-xs rounded border border-pm-accent text-pm-accent"
            >
              Try again
            </button>
            <button
              type="button"
              onClick={onGiveUp}
              className="px-4 py-2 text-xs rounded border border-pm-border text-pm-muted"
            >
              Back
            </button>
          </div>
        </>
      )}
    </div>
  );
}

export function readPairingInviteCode(): string | null {
  return parsePairingInvitePath(window.location.pathname);
}

export function clearPairingInviteFromUrl(): void {
  if (parsePairingInvitePath(window.location.pathname)) {
    window.history.replaceState(null, '', '/');
  }
}
