// Sheet shown in a pop-out pane window when its X is pressed while the agent is still working.
// Rust holds the close (pane_windows::on_close_requested) and emits CONFIRM_CLOSE_EVENT.
import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { CONFIRM_CLOSE_EVENT } from '../lib/close-policy';
import '../styles/close-confirm.css';

export interface ClosePaneApi {
  onConfirmRequest(cb: (paneId: string) => void): Promise<() => void>;
  stopAndClose(paneId: string): Promise<void>;
}

export const closePaneApi: ClosePaneApi = {
  onConfirmRequest: async (cb) => {
    const un = await listen<{ pane_id: string }>(CONFIRM_CLOSE_EVENT, (e) => cb(e.payload.pane_id));
    return un;
  },
  stopAndClose: (paneId) => invoke<void>('confirm_pane_window_close', { paneId }),
};

export function ClosePaneConfirm({
  paneId,
  agentLabel,
  api = closePaneApi,
}: {
  paneId: string;
  agentLabel: string;
  api?: ClosePaneApi;
}) {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let off: (() => void) | null = null;
    let disposed = false;
    void api
      .onConfirmRequest((id) => {
        if (id === paneId) setOpen(true);
      })
      .then((un) => {
        if (disposed) un();
        else off = un;
      })
      .catch(() => undefined); // not running inside Tauri
    return () => {
      disposed = true;
      off?.();
    };
  }, [api, paneId]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [open]);

  if (!open) return null;
  return (
    <div className="pm-close-scrim" onMouseDown={() => setOpen(false)}>
      <div
        className="pm-close-sheet"
        role="alertdialog"
        aria-labelledby="pm-close-title"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <h2 id="pm-close-title">{agentLabel} is still working.</h2>
        <p>Close and stop it?</p>
        <div className="pm-close-actions">
          <button type="button" className="pm-close-btn" autoFocus onClick={() => setOpen(false)}>
            Keep open
          </button>
          <button
            type="button"
            className="pm-close-btn pm-close-btn--primary"
            disabled={busy}
            onClick={() => {
              setBusy(true);
              void api.stopAndClose(paneId).catch(() => {
                setBusy(false);
                setOpen(false);
              });
            }}
          >
            Stop and close
          </button>
        </div>
      </div>
    </div>
  );
}
