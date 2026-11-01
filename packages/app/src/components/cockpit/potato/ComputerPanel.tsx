// The Computer panel (the Computer tab of the bot panel): the bot's sandbox screen in an iframe, who has it ("<Bot> is using it", "You have
// control", "Needs you") and the buttons Take over / Hand back / Skip. Nothing is drawn while the Labs switches are off
// (the app says so in the screen state). The iframe address carries the screen password in its fragment; it goes to the
// iframe and nowhere else (never shown, logged or put in the page). Status comes from the viewer's postMessage, believed
// only from the iframe's own window and origin. No colour.
import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { plainError } from '../../../lib/potato';
import {
  headerWords,
  originOf,
  parseViewerMessage,
  viewerNote,
  type ScreenLink,
  type ScreenMode,
  type ScreenState,
  type ScreenTransport,
  type ViewerMessage,
} from '../../../lib/potatoScreen';
import '../../../styles/cockpit-potato-panel.css';

/** While the person works in the control view, the lease is kept alive this often (it lasts 15 minutes). */
export const KEEP_EVERY_MS = 30_000;
/** A link is renewed this long before it runs out. */
const RENEW_BEFORE_MS = 60_000;
/** The viewer says it failed after 15 s; if it says nothing at all, the panel says so after this long. */
export const SILENT_VIEWER_MS = 20_000;

export interface ComputerPanelProps {
  bot: { id: string; name: string };
  transport: ScreenTransport;
  /** Said when a button fails. */
  onNotify?: (text: string) => void;
  /** Drawn instead of nothing while the Labs switches are off (the Computer tab explains itself). */
  whenOff?: ReactNode;
}

export function ComputerPanel({ bot, transport, onNotify, whenOff = null }: ComputerPanelProps) {
  const [screen, setScreen] = useState<ScreenState | null>(null);
  const [open, setOpen] = useState(false);
  const [link, setLink] = useState<ScreenLink | null>(null);
  const [linkProblem, setLinkProblem] = useState<string | null>(null);
  const [viewer, setViewer] = useState<ViewerMessage>({ status: 'loading', reason: null });
  const [retry, setRetry] = useState(0);
  const [busy, setBusy] = useState(false);
  const frame = useRef<HTMLIFrameElement>(null);

  const enabled = screen?.enabled === true;
  const phase = screen?.phase ?? 'free';
  const mode: ScreenMode = phase === 'you' ? 'control' : 'view';

  const reload = useCallback(() => {
    void transport.state().then(setScreen, () => setScreen(null));
  }, [transport]);

  // The state: read now, and again whenever the app says it moved.
  useEffect(() => {
    reload();
    let off: (() => void) | undefined;
    let gone = false;
    void transport.onChanged?.(reload).then((unlisten) => {
      if (gone) unlisten();
      else off = unlisten;
    });
    return () => {
      gone = true;
      off?.();
    };
  }, [transport, reload]);

  // A bot asking for you opens the panel, so the card is not missed.
  useEffect(() => {
    if (phase === 'needs_you' && screen?.bot === bot.id) setOpen(true);
  }, [phase, screen?.bot, bot.id]);

  // The link: one per mode, made when the panel is open. A new mode (Take over, Hand back) or Retry makes a new one.
  useEffect(() => {
    if (!open || !enabled) {
      setLink(null);
      return;
    }
    let current = true;
    setLink(null);
    setLinkProblem(null);
    setViewer({ status: 'loading', reason: null });
    transport.link(bot.id, mode).then(
      (made) => {
        if (current) setLink(made);
      },
      (error) => {
        if (current) setLinkProblem(plainError(error, 'Could not open the screen.'));
      },
    );
    return () => {
      current = false;
    };
  }, [open, enabled, mode, bot.id, transport, retry]);

  // A link is renewed a minute before it runs out.
  useEffect(() => {
    if (!link) return;
    const wait = Math.max(1000, link.expiresMs - Date.now() - RENEW_BEFORE_MS);
    const timer = setTimeout(() => setRetry((n) => n + 1), wait);
    return () => clearTimeout(timer);
  }, [link]);

  // A viewer that never reports (its page did not load, or the allowed origins did not match) is called failed.
  const waiting = !!link && (viewer.status === 'loading' || viewer.status === 'connecting');
  useEffect(() => {
    if (!waiting) return;
    const timer = setTimeout(() => setViewer({ status: 'failed', reason: 'timeout' }), SILENT_VIEWER_MS);
    return () => clearTimeout(timer);
  }, [waiting, link]);

  // The viewer's status. Only the iframe's own window, from the origin of the address we gave it, is believed.
  const expected = link ? originOf(link.src) : null;
  useEffect(() => {
    if (!expected) return;
    const onMessage = (event: MessageEvent) => {
      if (event.origin !== expected || !frame.current || event.source !== frame.current.contentWindow) return;
      const message = parseViewerMessage(event.data);
      if (message) setViewer(message);
    };
    window.addEventListener('message', onMessage);
    return () => window.removeEventListener('message', onMessage);
  }, [expected]);

  // While the person is working in the screen (the iframe has the keyboard), the lease is kept.
  const controlling = open && link?.mode === 'control' && phase === 'you';
  useEffect(() => {
    if (!controlling) return;
    const timer = setInterval(() => {
      if (document.activeElement === frame.current) void transport.keep().catch(() => undefined);
    }, KEEP_EVERY_MS);
    return () => clearInterval(timer);
  }, [controlling, transport]);

  const press = async (call: () => Promise<ScreenState>, opens = false) => {
    if (busy) return;
    setBusy(true);
    try {
      setScreen(await call());
      if (opens) setOpen(true);
    } catch (error) {
      onNotify?.(plainError(error, 'That did not work. Try again.'));
      reload();
    } finally {
      setBusy(false);
    }
  };

  if (!enabled || !screen) return <>{whenOff}</>;

  const note = linkProblem ?? (link ? viewerNote(viewer.status, viewer.reason) : open ? 'Connecting…' : null);
  const failed = linkProblem !== null || viewer.status === 'failed' || viewer.status === 'disconnected';
  const asking = phase === 'needs_you';

  return (
    <section className="ck-potato-computer" aria-label="Computer">
      <header className="ck-potato-computer-head">
        <h2 className="ck-potato-computer-title">Computer</h2>
        <p className={`ck-potato-computer-state is-${phase}`} role="status">
          {headerWords(screen, bot.id, bot.name)}
        </p>
        <div className="ck-potato-computer-actions">
          {(phase === 'bot' || asking || phase === 'free') && (
            <button type="button" className="ck-btn ck-btn--secondary" disabled={busy} onClick={() => void press(() => transport.takeOver(), true)}>
              Take over
            </button>
          )}
          {asking && screen.cardId && (
            <button type="button" className="ck-btn ck-btn--secondary" disabled={busy} onClick={() => void press(() => transport.skip(screen.cardId!))}>
              Skip
            </button>
          )}
          {phase === 'you' && (
            <button type="button" className="ck-btn ck-btn--secondary" disabled={busy} onClick={() => void press(() => transport.handBack())}>
              Hand back
            </button>
          )}
          <button type="button" className="ck-btn ck-btn--secondary" aria-expanded={open} onClick={() => setOpen((v) => !v)}>
            {open ? 'Hide' : 'Show'}
          </button>
        </div>
      </header>
      {asking && screen.reason && <p className="ck-sub ck-potato-computer-reason">{screen.reason}</p>}
      {open && (
        <div className="ck-potato-computer-view">
          {link && (
            <iframe
              key={`${link.mode}-${retry}`}
              ref={frame}
              className="ck-potato-computer-frame"
              title={`${bot.name}'s computer`}
              src={link.src}
              sandbox="allow-scripts allow-same-origin"
              allow="clipboard-read; clipboard-write"
            />
          )}
          {note && (
            <div className="ck-potato-computer-note" role="status">
              <span>{note}</span>
              {failed && (
                <button type="button" className="ck-btn ck-btn--secondary" onClick={() => setRetry((n) => n + 1)}>
                  Retry
                </button>
              )}
            </div>
          )}
        </div>
      )}
    </section>
  );
}
