// The global Ask bar UI. Works with any AskbarApi; the default is the Tauri one from lib/askbar.ts.
import { useCallback, useEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type ReactNode } from 'react';
import { tauriAskbarApi, clipboardChipLabel, type AskbarAgent, type AskbarApi, type AskbarClipboard, type AskResult } from '../../lib/askbar';
import { splitBlocks } from '../../lib/askbar-text';
import { DotSpinner } from '../cockpit/DotSpinner';

/** `code` between backticks shows as code, without the backticks. */
function inlineCode(text: string): ReactNode[] {
  return text.split(/(`[^`\r\n]+`)/g).map((part, i) =>
    part.length > 2 && part.startsWith('`') && part.endsWith('`') ? (
      <code key={i} className="askbar-inline">
        {part.slice(1, -1)}
      </code>
    ) : (
      part
    ),
  );
}

type View = 'empty' | 'running' | 'answer' | 'error';

interface LastRequest {
  agent: string;
  question: string;
  clipboard: string | null;
}

const FALLBACK_ERROR = 'Something went wrong.';
const NO_AGENT = 'No agent is set up to answer quick questions. Open Settings > Agents.';

function messageOf(e: unknown): string {
  if (typeof e === 'string' && e.trim()) return e;
  if (e && typeof e === 'object' && 'message' in e) {
    const message = (e as { message: unknown }).message;
    if (typeof message === 'string' && message.trim()) return message;
  }
  return FALLBACK_ERROR;
}

function newId(): string {
  const c = (globalThis as { crypto?: { randomUUID?: () => string } }).crypto;
  if (c?.randomUUID) return c.randomUUID();
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
}

/** Run a call that may be sync or async and may reject; the result does not matter to the bar. */
function ignore(p: unknown): void {
  void Promise.resolve(p).catch(() => {});
}

async function attempt<T>(f: () => T | Promise<T>, fallback: T): Promise<T> {
  try {
    return await f();
  } catch {
    return fallback;
  }
}

export function AskBar({ api = tauriAskbarApi }: { api?: AskbarApi }): JSX.Element {
  const [agents, setAgents] = useState<AskbarAgent[]>([]);
  /** The attached clipboard. Null until the user attaches it (or the setting attaches it on open). */
  const [clip, setClip] = useState<AskbarClipboard | null>(null);
  const [clipOn, setClipOn] = useState(false);
  /** The last attach found nothing to attach; the chip says so. */
  const [clipEmpty, setClipEmpty] = useState(false);
  const [agent, setAgent] = useState<string | null>(null);
  const [question, setQuestion] = useState('');
  const [view, setView] = useState<View>('empty');
  const [result, setResult] = useState<AskResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [menuOpen, setMenuOpen] = useState(false);
  const [copied, setCopied] = useState(false);

  const inputRef = useRef<HTMLInputElement>(null);
  const agentWrapRef = useRef<HTMLDivElement>(null);
  /** ask_id of the ask in flight (or the last one); results for other ids are ignored. */
  const latestAsk = useRef<string | null>(null);
  /** What the shown answer was asked with, for "Continue in a pane". */
  const lastRequest = useRef<LastRequest | null>(null);

  const agentLabel = agents.find((a) => a.id === agent)?.label ?? agent ?? '';
  const agentReady = agents.find((a) => a.id === agent)?.ready ?? false;

  /** Clear the bar and reload agents and settings. The clipboard is read here only when the setting attaches it by default. */
  const refresh = useCallback(async () => {
    latestAsk.current = null;
    lastRequest.current = null;
    setQuestion('');
    setResult(null);
    setError(null);
    setCopied(false);
    setMenuOpen(false);
    setView('empty');
    ignore(api.resize(false));
    inputRef.current?.focus();

    const [list, settings] = await Promise.all([
      attempt(() => api.agents(), [] as AskbarAgent[]),
      attempt(() => api.settingsGet(), null),
    ]);
    const attachByDefault = settings?.attach_clipboard_default === true;
    const clipNow = attachByDefault ? await attempt(() => api.clipboard(), null) : null;
    const listed = (id: string | null | undefined): string | null => (id && list.some((a) => a.id === id) ? id : null);

    setAgents(list);
    const attach = !!clipNow && clipNow.text.trim().length > 0;
    setClip(attach ? clipNow : null);
    setClipOn(attach);
    setClipEmpty(false);
    setAgent(listed(settings?.default_agent) ?? listed(settings?.last_agent) ?? list[0]?.id ?? null);
    if (list.length === 0) {
      setError(NO_AGENT);
      setView('error');
    }
    inputRef.current?.focus();
  }, [api]);

  /** An answer for an old question, pushed while the bar was hidden or from the notice list. */
  const showResult = useCallback((r: AskResult) => {
    latestAsk.current = null;
    lastRequest.current = { agent: r.agent, question: r.question, clipboard: null };
    setAgents((cur) => (cur.some((a) => a.id === r.agent) ? cur : [...cur, { id: r.agent, label: r.agent_label, ready: false }]));
    setAgent(r.agent);
    setQuestion(r.question);
    setMenuOpen(false);
    setCopied(false);
    if (r.ok) {
      setResult(r);
      setError(null);
      setView('answer');
    } else {
      setResult(null);
      setError(messageOf(r.error));
      setView('error');
    }
  }, []);

  // Mount: load everything and subscribe to the shell's events.
  useEffect(() => {
    let disposed = false;
    const offs: (() => void)[] = [];
    const track = (p: Promise<() => void>) => {
      void Promise.resolve(p)
        .then((off) => (disposed ? off() : offs.push(off)))
        .catch(() => {});
    };
    void refresh();
    track(api.onShown(() => void refresh()));
    track(api.onShowResult(showResult));
    return () => {
      disposed = true;
      offs.forEach((off) => off());
    };
  }, [api, refresh, showResult]);

  // The window grows for anything but the bare input line (and for the agent menu).
  useEffect(() => {
    ignore(api.resize(view !== 'empty' || menuOpen));
  }, [api, view, menuOpen]);

  // Esc closes the agent menu first, then hides the bar.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      if (menuOpen) setMenuOpen(false);
      else ignore(api.hide());
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [api, menuOpen]);

  // Click outside the agent menu closes it.
  useEffect(() => {
    if (!menuOpen) return;
    const onDown = (e: MouseEvent) => {
      if (!agentWrapRef.current?.contains(e.target as Node)) setMenuOpen(false);
    };
    document.addEventListener('mousedown', onDown);
    return () => document.removeEventListener('mousedown', onDown);
  }, [menuOpen]);

  // "Copied" reverts after 1.5s.
  useEffect(() => {
    if (!copied) return;
    const t = window.setTimeout(() => setCopied(false), 1500);
    return () => window.clearTimeout(t);
  }, [copied]);

  /** "+ Clipboard": the only place the clipboard is read when the setting is off. */
  const attachClipboard = async () => {
    const next = await attempt(() => api.clipboard(), null);
    if (next && next.text.trim().length > 0) {
      setClip(next);
      setClipOn(true);
      setClipEmpty(false);
    } else {
      setClip(null);
      setClipOn(false);
      setClipEmpty(true);
    }
  };

  const detachClipboard = () => {
    setClip(null);
    setClipOn(false);
    setClipEmpty(false);
  };

  const submit = async () => {
    const q = question.trim();
    if (!q || view === 'running' || !agent) return;
    const id = newId();
    const req: LastRequest = { agent, question: q, clipboard: clipOn && clip ? clip.text : null };
    latestAsk.current = id;
    lastRequest.current = req;
    setResult(null);
    setError(null);
    setCopied(false);
    setView('running');
    try {
      const r = await api.ask({ ask_id: id, question: q, agent: req.agent, clipboard: req.clipboard });
      if (latestAsk.current !== id || r.ask_id !== id) return;
      if (r.ok) {
        setResult(r);
        setView('answer');
      } else {
        setError(messageOf(r.error));
        setView('error');
      }
    } catch (e) {
      if (latestAsk.current !== id) return;
      setError(messageOf(e));
      setView('error');
    }
  };

  const onInputKey = (e: ReactKeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter' && !e.nativeEvent.isComposing) {
      e.preventDefault();
      void submit();
    }
  };

  const copy = async () => {
    if (!result?.answer) return;
    try {
      await navigator.clipboard.writeText(result.answer);
      setCopied(true);
    } catch {
      // The clipboard refused the write; leave the label alone.
    }
  };

  const continueInPane = async () => {
    const req = lastRequest.current;
    if (!req) return;
    try {
      await api.continueInPane(req.agent, req.question, req.clipboard);
    } catch (e) {
      setError(messageOf(e));
      setView('error');
      return;
    }
    ignore(api.hide());
  };

  const tryAgain = () => {
    setError(null);
    setView('empty');
    inputRef.current?.focus();
  };

  const chooseAgent = (id: string) => {
    setAgent(id);
    setMenuOpen(false);
  };

  const blocks = view === 'answer' && result ? splitBlocks(result.answer ?? '') : [];
  const errorText = view === 'error' ? error ?? FALLBACK_ERROR : null;

  return (
    <div className="askbar" data-state={view}>
      <div className="askbar-row">
        <div className="askbar-agent" ref={agentWrapRef}>
          <button
            type="button"
            className="askbar-chip"
            aria-haspopup="menu"
            aria-expanded={menuOpen}
            disabled={agents.length === 0}
            onClick={() => setMenuOpen((open) => !open)}
          >
            <span className="askbar-chip-label">{agentLabel || 'No agent'}</span>
            {agentReady && (
              <span className="askbar-chip-ready" aria-hidden="true">
                •
              </span>
            )}
          </button>
          {menuOpen && (
            <div className="askbar-menu" role="menu">
              {agents.map((a) => (
                <button
                  key={a.id}
                  type="button"
                  role="menuitemradio"
                  aria-checked={a.id === agent}
                  className="askbar-menu-row"
                  onClick={() => chooseAgent(a.id)}
                >
                  <span className="askbar-menu-label">{a.label}</span>
                  {a.ready && <span className="askbar-menu-ready">Ready</span>}
                </button>
              ))}
            </div>
          )}
        </div>

        <input
          ref={inputRef}
          className="askbar-input"
          placeholder="Ask anything…"
          aria-label="Question"
          autoFocus
          autoComplete="off"
          spellCheck={false}
          value={question}
          onChange={(e) => setQuestion(e.target.value)}
          onKeyDown={onInputKey}
        />

        {clipOn && clip ? (
          <span className="askbar-clip-wrap">
            <span className="askbar-clip">{clipboardChipLabel(clip)}</span>
            <button type="button" className="askbar-clip-x" aria-label="Detach clipboard" onClick={detachClipboard}>
              ×
            </button>
          </span>
        ) : (
          <button type="button" className="askbar-clip is-off" onClick={() => void attachClipboard()}>
            {clipEmpty ? 'Clipboard empty' : '+ Clipboard'}
          </button>
        )}
      </div>

      {view === 'running' && (
        <div className="askbar-status">
          <DotSpinner status="running" />
          <span>{`Asking ${agentLabel}…`}</span>
        </div>
      )}

      {view === 'answer' && result && (
        <>
          <div className="askbar-answer">
            {blocks.map((b, i) =>
              b.kind === 'code' ? (
                <pre key={i} className="askbar-code" data-lang={b.lang || undefined}>
                  {b.text}
                </pre>
              ) : (
                <p key={i} className="askbar-text">
                  {inlineCode(b.text)}
                </p>
              ),
            )}
          </div>
          <div className="askbar-actions">
            <button type="button" className="askbar-btn" onClick={() => void copy()}>
              {copied ? 'Copied' : 'Copy'}
            </button>
            <button type="button" className="askbar-btn askbar-btn--primary" onClick={() => void continueInPane()}>
              Open as an agent
            </button>
            <button type="button" className="askbar-btn" onClick={() => ignore(api.hide())}>
              Close
            </button>
          </div>
        </>
      )}

      {errorText !== null && (
        <>
          <p className="askbar-error" role="alert">
            {errorText}
          </p>
          <div className="askbar-actions">
            {agents.length > 0 && (
              <button type="button" className="askbar-btn" onClick={tryAgain}>
                Try again
              </button>
            )}
            <button type="button" className="askbar-btn askbar-btn--primary" onClick={() => ignore(api.hide())}>
              Close
            </button>
          </div>
        </>
      )}
    </div>
  );
}
