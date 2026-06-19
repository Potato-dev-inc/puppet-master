import { parsePairingInviteFromQr, parsePairingQrPayload } from '@puppet-master/shared';
import { useCallback, useEffect, useId, useRef, useState } from 'react';
import type { Html5Qrcode } from 'html5-qrcode';

interface Props {
  open: boolean;
  onClose: () => void;
  onScan: (payloadJson: string) => void;
  onScanInvite?: (pairingCode: string) => void;
}

function isPairingQrPayload(text: string): boolean {
  try {
    parsePairingQrPayload(text);
    return true;
  } catch {
    return false;
  }
}

async function stopScanner(scanner: Html5Qrcode | null): Promise<void> {
  if (!scanner) return;
  try {
    if (scanner.isScanning) await scanner.stop();
    scanner.clear();
  } catch {
    /* camera may already be released */
  }
}

/** Full-screen camera scanner for PWA pairing QR (JSON payload, not a URL). */
export function PairingQrScanner({ open, onClose, onScan, onScanInvite }: Props) {
  const readerId = useId().replace(/:/g, '');
  const scannerRef = useRef<Html5Qrcode | null>(null);
  const handledRef = useRef(false);
  const [cameraErr, setCameraErr] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);

  const release = useCallback(async () => {
    await stopScanner(scannerRef.current);
    scannerRef.current = null;
    handledRef.current = false;
  }, []);

  const startCamera = useCallback(async () => {
    setCameraErr(null);
    setStarting(true);
    handledRef.current = false;
    await stopScanner(scannerRef.current);
    scannerRef.current = null;

    try {
      const { Html5Qrcode } = await import('html5-qrcode');
      const scanner = new Html5Qrcode(readerId, { verbose: false });
      scannerRef.current = scanner;

      await scanner.start(
        { facingMode: 'environment' },
        { fps: 10, qrbox: { width: 240, height: 240 }, aspectRatio: 1 },
        (decodedText) => {
          if (handledRef.current) return;
          const inviteCode = parsePairingInviteFromQr(decodedText);
          if (inviteCode && onScanInvite) {
            handledRef.current = true;
            void (async () => {
              await stopScanner(scanner);
              scannerRef.current = null;
              onScanInvite(inviteCode);
            })();
            return;
          }
          if (!isPairingQrPayload(decodedText)) return;
          handledRef.current = true;
          void (async () => {
            await stopScanner(scanner);
            scannerRef.current = null;
            onScan(decodedText.trim());
          })();
        },
        () => {},
      );
    } catch (e) {
      setCameraErr(e instanceof Error ? e.message : String(e));
    } finally {
      setStarting(false);
    }
  }, [onScan, onScanInvite, readerId]);

  useEffect(() => {
    if (!open) {
      void release();
      setCameraErr(null);
      return;
    }
    void startCamera();
    return () => {
      void release();
    };
  }, [open, release, startCamera]);

  if (!open) return null;

  return (
    <div className="fixed inset-0 z-50 flex flex-col bg-black/90">
      <div className="flex items-center justify-between px-4 py-3 border-b border-pm-border/40">
        <h2 className="text-sm font-semibold text-zinc-100">Scan pairing QR</h2>
        <button
          type="button"
          onClick={() => {
            void release().then(onClose);
          }}
          className="px-3 py-1 text-xs rounded border border-pm-border text-pm-muted hover:text-zinc-200"
        >
          Cancel
        </button>
      </div>

      <div className="flex-1 flex flex-col items-center justify-center p-4 gap-3 min-h-0">
        <p className="text-xs text-pm-muted text-center max-w-xs">
          Point your camera at the QR in desktop Settings → Mobile pairing.
        </p>
        <div
          id={readerId}
          className="w-full max-w-sm overflow-hidden rounded-xl border border-pm-border bg-pm-bg [&_video]:rounded-xl"
        />
        {starting && <p className="text-xs text-pm-muted">Starting camera…</p>}
        {cameraErr && (
          <div className="w-full max-w-sm space-y-2 text-center">
            <p className="text-xs text-pm-err">{cameraErr}</p>
            <button
              type="button"
              onClick={() => void startCamera()}
              className="px-4 py-2 text-xs rounded border border-pm-accent text-pm-accent"
            >
              Try again
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
