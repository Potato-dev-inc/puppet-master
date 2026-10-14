// Bottom sheets of the mobile PWA: "New agent" (opens a pane on the PC) and the "⋯" menu
// (connection, close the current agent, input box settings, classic view link).
import { useEffect, useRef, useState, type ReactNode } from 'react';
import type { AgentType } from '@puppet-master/shared';
import type { LaunchEntry } from '../../lib/agentRegistry';
import type { PublicSettings } from '../../lib/bridge-settings';
import type { BridgeLinkStats } from '../../lib/bridge-sse';
import { DotSpinner } from '../cockpit/DotSpinner';
import { NotificationsToggle } from './NotificationsToggle';

/** Past this drag distance (or this speed) a swipe down closes the sheet. */
const SWIPE_CLOSE_PX = 110;
const SWIPE_CLOSE_VELOCITY = 0.6; // px per ms
const SHEET_CLOSE_MS = 240;

/**
 * Bottom sheet: scrim, grabber and title bar. Drag the grabber/title down (or fling it) to close;
 * tap the scrim, press Escape or "Done" for the same. The body scrolls on its own.
 */
export function Sheet({ title, onClose, children }: { title: string; onClose: () => void; children: ReactNode }) {
  const sheetRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<{ y: number; t: number; dy: number; v: number } | null>(null);
  const [dragging, setDragging] = useState(false);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  const move = (dy: number, settle: boolean) => {
    const el = sheetRef.current;
    if (!el) return;
    el.classList.toggle('ph-sheet--settling', settle);
    el.style.transform = dy > 0 ? `translateY(${dy}px)` : '';
  };

  const onPointerDown = (e: React.PointerEvent) => {
    if ((e.target as Element).closest('button')) return;
    dragRef.current = { y: e.clientY, t: performance.now(), dy: 0, v: 0 };
    setDragging(true);
    (e.currentTarget as HTMLElement).setPointerCapture?.(e.pointerId);
  };
  const onPointerMove = (e: React.PointerEvent) => {
    const d = dragRef.current;
    if (!d) return;
    const now = performance.now();
    const dy = Math.max(0, e.clientY - d.y);
    d.v = (dy - d.dy) / Math.max(1, now - d.t);
    d.dy = dy;
    d.t = now;
    move(dy, false);
  };
  const onPointerEnd = () => {
    const d = dragRef.current;
    dragRef.current = null;
    setDragging(false);
    if (!d) return;
    if (d.dy > SWIPE_CLOSE_PX || d.v > SWIPE_CLOSE_VELOCITY) {
      move(sheetRef.current?.offsetHeight ?? 600, true);
      window.setTimeout(onClose, SHEET_CLOSE_MS);
    } else {
      move(0, true);
    }
  };

  return (
    <div className="ph-scrim" onClick={onClose}>
      <div
        ref={sheetRef}
        role="dialog"
        aria-label={title}
        aria-modal="true"
        onClick={(e) => e.stopPropagation()}
        className={`ph-sheet${dragging ? ' ph-sheet--dragging' : ''}`}
      >
        <div
          className="ph-sheet-top"
          data-sheet-drag
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerEnd}
          onPointerCancel={onPointerEnd}
        >
          <div className="ph-grabber" aria-hidden="true" />
          <div className="ph-sheet-h">
            <b>{title}</b>
            <button type="button" aria-label="Close" onClick={onClose} className="ph-btn ph-btn--plain">
              Done
            </button>
          </div>
        </div>
        <div className="ph-sheet-body">{children}</div>
      </div>
    </div>
  );
}

/** iOS-style action sheet: a message, one confirm action and Cancel. */
export function ConfirmSheet({
  title,
  message,
  confirmLabel,
  onConfirm,
  onCancel,
}: {
  title: string;
  message: string;
  confirmLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  return (
    <div className="ph-action" onClick={onCancel}>
      <div role="alertdialog" aria-label={title} onClick={(e) => e.stopPropagation()}>
        <div className="ph-action-card">
          <div className="ph-action-msg">
            <b>{title}</b>
            {message}
          </div>
          <button type="button" className="ph-action-btn ph-action-btn--strong" onClick={onConfirm}>
            {confirmLabel}
          </button>
        </div>
        <div className="ph-action-card ph-action-cancel">
          <button type="button" className="ph-action-btn" onClick={onCancel}>
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}

export function NewAgentSheet({
  entries,
  onClose,
  onSpawn,
  spawning,
  error,
}: {
  /** null while the agent list loads. */
  entries: LaunchEntry[] | null;
  onClose: () => void;
  onSpawn: (agentType: AgentType) => void;
  spawning: boolean;
  error: string | null;
}) {
  return (
    <Sheet title="Open an agent on your PC" onClose={onClose}>
      {!entries && <div className="ph-loading">Loading agents…</div>}
      {entries && (
        <ul className="ph-group" aria-label="Agents">
          {entries.map((entry) => (
            <li key={entry.type}>
              <button
                type="button"
                disabled={spawning || entry.disabled}
                title={entry.disabledReason}
                onClick={() => onSpawn(entry.type)}
                className="ph-row"
                style={entry.disabled ? { opacity: 0.4 } : undefined}
              >
                <span className="ph-txt">
                  <span className="ph-t">{entry.label}</span>
                  {entry.experimental && <span className="ph-s">Experimental</span>}
                  {entry.disabledReason && <span className="ph-s">{entry.disabledReason}</span>}
                </span>
                {!entry.disabled && (
                  <span className="ph-chev" aria-hidden="true">
                    <svg viewBox="0 0 24 24">
                      <path d="m9 5 7 7-7 7" />
                    </svg>
                  </span>
                )}
              </button>
            </li>
          ))}
        </ul>
      )}
      {spawning && (
        <div className="ph-foot" role="status" style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <DotSpinner status="running" />
          Starting…
        </div>
      )}
      {error && <div className="ph-foot ph-foot--err" role="alert">{error}</div>}
    </Sheet>
  );
}

/** Keystroke-to-echo delay measured by the phone: the last one and the median of recent ones. */
export interface TypingEchoStats {
  lastMs: number | null;
  medianMs: number | null;
}

const fmtMs = (ms: number | null | undefined): string => (ms === null || ms === undefined ? '—' : `${Math.round(ms)} ms`);

const reconnectsText = (n: number): string | null => (n <= 0 ? null : n === 1 ? 'reconnected once' : `reconnected ${n} times`);

/**
 * One plain-text line describing the link, in words a person reads ("typing delay", "reconnects") rather than
 * transport names. Numbers that have not been measured yet are left out instead of shown as dashes.
 */
export function connectionLine(link: BridgeLinkStats | null, echo: TypingEchoStats | null): string {
  const delay =
    echo?.lastMs === null || echo?.lastMs === undefined
      ? null
      : `typing delay ${fmtMs(echo.lastMs)}${echo.medianMs === null ? '' : ` (usually ${fmtMs(echo.medianMs)})`}`;
  const parts: (string | null)[] = [];
  if (!link) parts.push('Connecting…', delay);
  else if (link.mode === 'polling') parts.push('Slow connection, retrying', delay);
  else if (link.mode === 'stream') parts.push('Connected on this network', delay);
  else if (link.mode === 'connecting') parts.push('Connecting…', delay, reconnectsText(link.reconnects));
  else
    parts.push(
      `Connected · ${fmtMs(link.rttMs)} round trip`,
      delay,
      reconnectsText(link.reconnects),
    );
  return parts.filter(Boolean).join(' · ');
}

export function MenuSheet({
  onClose,
  bridgeUrl,
  bridgeReady,
  settings,
  onPatchSettings,
  link,
  echo,
  activeLabel,
  onCloseActive,
  onEditConnection,
  onStopAll,
}: {
  onClose: () => void;
  bridgeUrl: string;
  bridgeReady: boolean;
  settings: PublicSettings;
  onPatchSettings: (patch: Partial<PublicSettings>) => void;
  /** Live link stats from the event stream; null before it has subscribed. */
  link: BridgeLinkStats | null;
  echo: TypingEchoStats | null;
  /** Label of the open agent tab, null on All. */
  activeLabel: string | null;
  onCloseActive: () => void;
  onEditConnection: () => void;
  /** Panic button: interrupt every running agent on the PC. */
  onStopAll?: () => void;
}) {
  const setDelay = (value: number) => {
    const ms = Number.isFinite(value) ? (value <= 0 ? 0 : Math.min(1000, Math.max(50, Math.round(value)))) : 250;
    onPatchSettings({ mobile_input_delay_ms: ms });
  };
  const [confirm, setConfirm] = useState<'stop' | 'close' | null>(null);
  return (
    <>
      <Sheet title="Puppet Master" onClose={onClose}>
        <h2 className="ph-sec">Connection</h2>
        <div className="ph-group">
          <div className="ph-row">
            <span className={`ph-conn${bridgeReady ? '' : ' ph-conn--off'}`} aria-hidden="true" />
            <span className="ph-rowtext">{bridgeReady ? 'Connected' : 'Connecting'}</span>
            <button type="button" onClick={onEditConnection} className="ph-btn ph-btn--plain" style={{ marginRight: -10 }}>
              Change
            </button>
          </div>
          <div className="ph-row" style={{ minHeight: 0 }}>
            <span className="ph-value" style={{ maxWidth: '100%' }} title={bridgeUrl}>{bridgeUrl}</span>
          </div>
        </div>
        <p className="ph-foot ph-foot--mono" aria-label="Connection">
          {connectionLine(link, echo)}
        </p>

        <h2 className="ph-sec">Notifications</h2>
        <NotificationsToggle baseUrl={bridgeUrl} bridgeReady={bridgeReady} />

        <h2 className="ph-sec">Terminal</h2>
        <div className="ph-group">
          <label className="ph-switch-row">
            <span className="ph-rowtext">Input box under the terminal</span>
            <input
              type="checkbox"
              className="ph-switch"
              checked={settings.mobile_input_visible ?? true}
              disabled={!bridgeReady}
              onChange={(e) => onPatchSettings({ mobile_input_visible: e.target.checked })}
            />
          </label>
          <label className="ph-switch-row" style={{ borderTop: '1px solid var(--sep)' }}>
            <span className="ph-rowtext">Keyboard resizes the agent</span>
            <input
              type="checkbox"
              className="ph-switch"
              checked={settings.mobile_keyboard_resizes_agent ?? false}
              disabled={!bridgeReady}
              onChange={(e) => onPatchSettings({ mobile_keyboard_resizes_agent: e.target.checked })}
            />
          </label>
          <label className="ph-switch-row" style={{ borderTop: '1px solid var(--sep)' }}>
            <span className="ph-rowtext">Typing buffer</span>
            <input
              type="number"
              min={0}
              max={1000}
              step={10}
              value={settings.mobile_input_delay_ms ?? 60}
              disabled={!bridgeReady}
              onChange={(e) => setDelay(Number(e.target.value))}
              className="ph-numfield"
              aria-label="Typing buffer in milliseconds"
            />
            <span className="ph-value">ms</span>
          </label>
        </div>
        <p className="ph-foot">
          Off: the agent keeps its size and the keyboard covers the top of the screen (swipe to see it). On: the agent redraws above the keyboard (Claude reprints each time).
        </p>

        {(onStopAll || activeLabel) && (
          <>
            <h2 className="ph-sec">Agents</h2>
            <div className="ph-group">
              {onStopAll && (
                <button type="button" disabled={!bridgeReady} onClick={() => setConfirm('stop')} className="ph-rowbtn ph-rowbtn--strong">
                  Stop all agents
                </button>
              )}
              {activeLabel && (
                <button
                  type="button"
                  onClick={() => setConfirm('close')}
                  className="ph-rowbtn ph-rowbtn--strong"
                  style={onStopAll ? { borderTop: '1px solid var(--sep)' } : undefined}
                >
                  Close {activeLabel}
                </button>
              )}
            </div>
          </>
        )}

        <div className="ph-group" style={{ marginTop: 22 }}>
          <a href="?pwa=legacy" className="ph-row">
            <span className="ph-rowtext">Classic mobile view (orchestrator)</span>
          </a>
        </div>
      </Sheet>
      {confirm === 'stop' && onStopAll && (
        <ConfirmSheet
          title="Stop all agents?"
          message="Every agent that is working gets an interrupt (Ctrl+C)."
          confirmLabel="Stop all"
          onCancel={() => setConfirm(null)}
          onConfirm={() => {
            setConfirm(null);
            onStopAll();
          }}
        />
      )}
      {confirm === 'close' && activeLabel && (
        <ConfirmSheet
          title={`Close ${activeLabel}?`}
          message="This stops the agent on your PC."
          confirmLabel={`Close ${activeLabel}`}
          onCancel={() => setConfirm(null)}
          onConfirm={() => {
            setConfirm(null);
            onCloseActive();
          }}
        />
      )}
    </>
  );
}
