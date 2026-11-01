// One viewer of the bot's sandbox screen: the link for a mode, its renewal, the status the viewer posts and the lease
// keep-alive. The preview card (view mode) and the full-window overlay (view or control mode) both use it, one at a
// time. The iframe address carries the screen password in its fragment; it goes to the iframe and nowhere else (never
// shown, logged or put in the page). Status is believed only from the iframe's own window and origin.
import { useCallback, useEffect, useRef, useState, type RefObject } from 'react';
import { plainError } from '../../../lib/potato';
import { originOf, parseViewerMessage, viewerNote, type ScreenLink, type ScreenMode, type ScreenTransport, type ViewerMessage } from '../../../lib/potatoScreen';

/** While the person works in the control view, the lease is kept alive this often (it lasts 15 minutes). */
export const KEEP_EVERY_MS = 30_000;
/** A link is renewed this long before it runs out. */
export const RENEW_BEFORE_MS = 60_000;
/** The viewer says it failed after 15 s; if it says nothing at all, the panel says so after this long. */
export const SILENT_VIEWER_MS = 20_000;

export interface ScreenLinkOptions {
  transport: ScreenTransport;
  botId: string;
  mode: ScreenMode;
  /** No link is made (and none is kept) while this is false. */
  active: boolean;
  /** Keep the lease while the person works in the frame (the control view). */
  keepLease?: boolean;
}

export interface ScreenLinkView {
  frame: RefObject<HTMLIFrameElement>;
  link: ScreenLink | null;
  viewer: ViewerMessage;
  /** The line over the screen while it is not showing, or null. */
  note: string | null;
  failed: boolean;
  /** The title of the window the bot is on, when the viewer says it. */
  title: string | null;
  /** Makes a new link. */
  retry: () => void;
  /** Bumped on each retry; used as part of the iframe key. */
  attempt: number;
}

export function useScreenLink({ transport, botId, mode, active, keepLease = false }: ScreenLinkOptions): ScreenLinkView {
  const [link, setLink] = useState<ScreenLink | null>(null);
  const [linkProblem, setLinkProblem] = useState<string | null>(null);
  const [viewer, setViewer] = useState<ViewerMessage>({ status: 'loading', reason: null });
  const [title, setTitle] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  const frame = useRef<HTMLIFrameElement>(null);

  // The link: one per mode, made while active. A new mode or Retry makes a new one.
  useEffect(() => {
    if (!active) {
      setLink(null);
      setLinkProblem(null);
      return;
    }
    let current = true;
    setLink(null);
    setLinkProblem(null);
    setTitle(null);
    setViewer({ status: 'loading', reason: null });
    transport.link(botId, mode).then(
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
  }, [active, mode, botId, transport, attempt]);

  // A link is renewed a minute before it runs out.
  useEffect(() => {
    if (!link) return;
    const wait = Math.max(1000, link.expiresMs - Date.now() - RENEW_BEFORE_MS);
    const timer = setTimeout(() => setAttempt((n) => n + 1), wait);
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
      if (!message) return;
      setViewer({ status: message.status, reason: message.reason });
      if (message.title) setTitle(message.title);
    };
    window.addEventListener('message', onMessage);
    return () => window.removeEventListener('message', onMessage);
  }, [expected]);

  // While the person is working in the screen (the iframe has the keyboard), the lease is kept.
  const keeping = keepLease && active && link?.mode === 'control';
  useEffect(() => {
    if (!keeping) return;
    const timer = setInterval(() => {
      if (document.activeElement === frame.current) void transport.keep().catch(() => undefined);
    }, KEEP_EVERY_MS);
    return () => clearInterval(timer);
  }, [keeping, transport]);

  const retry = useCallback(() => setAttempt((n) => n + 1), []);
  const note = linkProblem ?? (link ? viewerNote(viewer.status, viewer.reason) : active ? 'Connecting…' : null);
  const failed = linkProblem !== null || viewer.status === 'failed' || viewer.status === 'disconnected';
  return { frame, link, viewer, note, failed, title, retry, attempt };
}
