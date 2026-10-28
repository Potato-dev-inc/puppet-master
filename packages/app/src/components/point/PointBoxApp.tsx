// Root of the Point box window (its own window, label `pointbox`, route `index.html?pointbox=1`).
// Holds the state and makes the calls; the box itself is PointBox.
import { useCallback, useEffect, useRef, useState } from 'react';
import '../../styles/point-box.css';
import { pointApi, type PointState } from '../../lib/point-api';
import { PointBox } from './PointBox';

type PointApi = typeof pointApi;

const NO_STATE: PointState = { picks: [], targets: [], defaultTarget: null, notice: null };
const NO_TEXT = 'Write what should change first.';
const NO_TARGET = 'Pick an agent pane to send to.';
const COPY_FAILED = 'Could not copy the text.';
const COPIED = 'Copied.';

function messageOf(e: unknown): string {
  if (typeof e === 'string' && e.trim()) return e;
  if (e && typeof e === 'object' && 'message' in e) {
    const message = (e as { message: unknown }).message;
    if (typeof message === 'string' && message.trim()) return message;
  }
  return 'Something went wrong.';
}

export function PointBoxApp({ api = pointApi }: { api?: PointApi }): JSX.Element {
  const [state, setState] = useState<PointState>(NO_STATE);
  const [target, setTarget] = useState<string | null>(null);
  const [request, setRequest] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [info, setInfo] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // The session id of the last state (undefined before the first one). See apply.
  const sessionRef = useRef<number | null | undefined>(undefined);

  /**
   * Keep the chosen pane while it still exists; otherwise take the backend's default. A different session id
   * (a new session started, or the last one ended) means a new box: the text and the notes are cleared.
   * The same id keeps them, so an Esc-hidden box still has its text when the person picks again.
   */
  const apply = useCallback((next: PointState) => {
    setState(next);
    setTarget((cur) => (cur && next.targets.some((t) => t.paneId === cur) ? cur : next.defaultTarget ?? null));
    const id = next.session ?? null;
    if (sessionRef.current !== undefined && sessionRef.current !== id) {
      setRequest('');
      setError(null);
      setInfo(null);
    }
    sessionRef.current = id;
  }, []);

  const load = useCallback(() => {
    api.state().then(apply, (e: unknown) => setError(messageOf(e)));
  }, [api, apply]);

  // Mount: transparent window background, the shell's state pushes, and the first state.
  useEffect(() => {
    document.documentElement.classList.add('point-box-root');
    const off = api.onState(apply);
    load();
    return () => {
      document.documentElement.classList.remove('point-box-root');
      off();
    };
  }, [api, apply, load]);

  // "Copied." goes away after 1.5 s.
  useEffect(() => {
    if (!info) return;
    const t = window.setTimeout(() => setInfo(null), 1500);
    return () => window.clearTimeout(t);
  }, [info]);

  /** The trimmed text, or null after saying why nothing can happen yet. */
  const readyText = (): string | null => {
    const text = request.trim();
    if (!text) {
      setError(NO_TEXT);
      return null;
    }
    if (!target) {
      setError(NO_TARGET);
      return null;
    }
    return text;
  };

  /**
   * The box is empty: no text, no picks, no notes. The next pick opens with a blank box. Runs first in
   * Discard, so the box is clear even when the shell is slow to answer.
   */
  const clearBox = () => {
    setRequest('');
    setError(null);
    setInfo(null);
    setState((s) => ({ ...s, picks: [] }));
  };

  /** After a send or an Edit in Chat the session is over: clear the box and reload the state. */
  const finished = () => {
    clearBox();
    load();
  };

  const send = async () => {
    const text = readyText();
    if (!text || !target || busy) return;
    setBusy(true);
    setError(null);
    try {
      await api.send(target, text);
      finished();
    } catch (e) {
      setError(messageOf(e));
    } finally {
      setBusy(false);
    }
  };

  const editInChat = async () => {
    const text = readyText();
    if (!text || !target || busy) return;
    setError(null);
    try {
      await api.editInChat(target, text);
      finished();
    } catch (e) {
      setError(messageOf(e));
    }
  };

  const copy = async () => {
    const text = readyText();
    if (!text || !target) return;
    let out: string;
    try {
      out = await api.compose(target, text);
    } catch (e) {
      setError(messageOf(e));
      return;
    }
    try {
      await navigator.clipboard.writeText(out);
      setError(null);
      setInfo(COPIED);
    } catch {
      setError(COPY_FAILED);
    }
  };

  const pointAnother = async () => {
    try {
      await api.arm();
      setError(null);
    } catch (e) {
      setError(messageOf(e));
    }
  };

  const remove = async (id: string) => {
    try {
      apply(await api.removePick(id));
    } catch (e) {
      setError(messageOf(e));
    }
  };

  const discard = async () => {
    clearBox();
    try {
      await api.discard();
    } catch (e) {
      setError(messageOf(e));
    }
    // Ask the shell again, so the box shows what it really holds (empty after a good discard).
    load();
  };

  const hide = () => {
    void Promise.resolve(api.hide()).catch(() => {});
  };

  const openBlocked = async () => {
    try {
      await api.openBlockedLink();
    } catch (e) {
      setError(messageOf(e));
    }
  };

  return (
    <PointBox
      state={state}
      request={request}
      onRequest={(text) => {
        setRequest(text);
        setError(null);
      }}
      target={target}
      onTarget={setTarget}
      onSend={() => void send()}
      onEditInChat={() => void editInChat()}
      onCopy={() => void copy()}
      onPointAnother={() => void pointAnother()}
      onRemove={(id) => void remove(id)}
      onDiscard={() => void discard()}
      onHide={hide}
      onOpenBlocked={() => void openBlocked()}
      error={error}
      info={info}
      busy={busy}
    />
  );
}

export default PointBoxApp;
