// "The brief <agent> got": the read-only text a Keep Going hand-over sent to the new agent. It shows what the
// agent was told, nothing more: no folder, no run ids. The text comes from `keepGoingBrief` (already redacted).
import { useEffect, useId, useRef, useState } from 'react';
import { Sheet } from './Sheet';

/** The size in plain units: bytes under 1 KB, else KB with one decimal (1 KB = 1024 bytes). */
export function briefSize(text: string): string {
  const bytes = new TextEncoder().encode(text).length;
  return bytes < 1024 ? `${bytes} B` : `${(bytes / 1024).toFixed(1)} KB`;
}

const COPIED_MS = 1500;

export function BriefSheet({
  agent,
  text,
  error,
  trimmed,
  onClose,
}: {
  /** The new agent's name in plain words, for the title ("The brief Fake B got"). */
  agent: string;
  /** The brief, or null while it loads. */
  text: string | null;
  /** A plain-English reason it could not load, or null. */
  error?: string | null;
  /**
   * The parts the brief builder had to shorten. Leave it out when the caller does not know: the size line
   * then shows the size alone rather than claim nothing was trimmed.
   */
  trimmed?: string[];
  onClose: () => void;
}) {
  const titleId = useId();
  const [copy, setCopy] = useState<'idle' | 'copied' | 'failed'>('idle');
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => () => {
    if (timer.current !== null) clearTimeout(timer.current);
  }, []);

  const copyText = async () => {
    if (text === null) return;
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      setCopy('failed');
      return;
    }
    setCopy('copied');
    if (timer.current !== null) clearTimeout(timer.current);
    timer.current = setTimeout(() => setCopy('idle'), COPIED_MS);
  };

  const sizeLine =
    text === null
      ? null
      : trimmed === undefined
        ? briefSize(text)
        : `${briefSize(text)} · trimmed: ${trimmed.length > 0 ? trimmed.join(', ') : 'none'}`;

  return (
    <Sheet onClose={onClose} role="dialog" labelledBy={titleId}>
      <div id={titleId} className="ck-title">
        The brief {agent} got
      </div>
      {sizeLine !== null && <div className="ck-sub">{sizeLine}</div>}
      {error ? (
        <p className="ck-sub" role="alert">
          {error}
        </p>
      ) : text === null ? (
        <p className="ck-sub">Loading the brief…</p>
      ) : (
        <pre
          tabIndex={0}
          aria-label={`The brief ${agent} got`}
          style={{
            margin: '10px 0 0',
            padding: 10,
            maxHeight: 320,
            overflow: 'auto',
            whiteSpace: 'pre-wrap',
            wordBreak: 'break-word',
            fontSize: 11,
            lineHeight: 1.45,
            borderRadius: 8,
            background: 'var(--fill)',
            color: 'var(--label)',
            fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Consolas, monospace',
          }}
        >
          {text}
        </pre>
      )}
      {copy === 'failed' && (
        <p className="ck-sub" role="alert">
          Could not copy. Select the text and copy it.
        </p>
      )}
      <div className="ck-sheet-actions">
        <button type="button" className="ck-btn ck-btn--secondary" disabled={text === null} onClick={() => void copyText()}>
          {copy === 'copied' ? 'Copied' : 'Copy'}
        </button>
        <button type="button" className="ck-btn ck-btn--primary" onClick={onClose}>
          Done
        </button>
      </div>
    </Sheet>
  );
}
