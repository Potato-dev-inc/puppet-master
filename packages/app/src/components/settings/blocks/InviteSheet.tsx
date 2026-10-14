// "Invite someone": the join link as a QR code, the one-time code, and the link to copy. The link and the
// code are sent separately on purpose. The team id is never shown (the link carries what it needs).
import QRCode from 'qrcode';
import { useEffect, useRef, useState } from 'react';
import type { InviteView, TeamApi } from '../../../lib/team';
import { Btn, Note } from '../settings-ui';

const errText = (err: unknown): string => (err instanceof Error ? err.message : String(err));
const clock = (ms: number): string => new Date(ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });

export function InviteSheet({ api, onClose }: { api: TeamApi; onClose: () => void }) {
  const [invite, setInvite] = useState<InviteView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    let cancelled = false;
    api
      .invite()
      .then((v) => !cancelled && setInvite(v))
      .catch((err) => !cancelled && setError(errText(err)));
    return () => {
      cancelled = true;
    };
  }, [api]);

  useEffect(() => {
    closeRef.current?.focus();
  }, []);

  useEffect(() => {
    if (!invite || !canvasRef.current) return;
    // The same look as the phone pairing code. A QR that cannot be drawn leaves the link and the code.
    QRCode.toCanvas(canvasRef.current, invite.link, { width: 168, margin: 2, color: { dark: '#e8e8e8', light: '#121212' } }).catch(() => {});
  }, [invite]);

  const copy = async () => {
    if (!invite) return;
    try {
      await navigator.clipboard.writeText(invite.link);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      setError("Couldn't copy. Select the link and copy it by hand.");
    }
  };

  return (
    <div
      className="st-scrim"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
      onKeyDown={(e) => {
        if (e.key === 'Escape') onClose();
      }}
    >
      <div className="st-sheet st-invite" role="dialog" aria-modal="true" aria-labelledby="st-invite-title">
        <h2 id="st-invite-title">Invite someone</h2>
        {!invite && !error && <p>Making an invite…</p>}
        {invite && (
          <>
            <div className="st-invite-top">
              <canvas ref={canvasRef} className="st-invite-qr" aria-label="Invite QR code" />
              <div className="st-invite-codebox">
                <small>Code</small>
                <div className="st-invite-code" aria-label="Invite code">
                  {invite.code}
                </div>
                <small>Expires at {clock(invite.expiresAtMs)}</small>
              </div>
            </div>
            <div className="st-invite-link">
              <input className="st-field is-mono" readOnly value={invite.link} aria-label="Invite link" onFocus={(e) => e.currentTarget.select()} />
              <Btn onClick={() => void copy()}>{copied ? 'Copied' : 'Copy'}</Btn>
            </div>
            <Note>Send the link and the code separately. The code works once, for 10 minutes. You'll be asked before anyone gets in.</Note>
          </>
        )}
        {error && <Note role="alert">{error}</Note>}
        <div className="st-sheet-actions">
          <button ref={closeRef} type="button" className="st-btn is-primary" onClick={onClose}>
            Done
          </button>
        </div>
      </div>
    </div>
  );
}
