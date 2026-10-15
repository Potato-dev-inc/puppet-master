import { Fragment, useState } from 'react';
import { lineKind } from './diff';
import { newLineNumbers, severityGlyph, severityLabel, type ReviewFinding, type Severity } from './review-diff';
import '../../styles/cockpit-second-opinion.css';

export interface AnnotatedDiffProps {
  file: string;
  lines: string[]; // diff body lines for this file (splitPatch output)
  findings: ReviewFinding[]; // already anchored to this file
  agentLabel: string | null; // original agent's name, e.g. "Claude Code"; null = cannot send
  onSend?: (file: string, line: number | null, text: string) => Promise<void>; // send a finding or a free comment to the original agent
  emptyText?: string;
}

const RANK: Record<Severity, number> = { high: 3, medium: 2, low: 1, info: 0 };

type SendState = { status: 'sending' } | { status: 'sent' } | { status: 'error'; message: string };

const errText = (err: unknown): string => (err instanceof Error ? err.message : String(err));

/**
 * A file's diff with the second opinion's findings pinned under the lines they point at, and a quiet
 * per-line comment affordance. Monochrome like the Changes viewer: severity is shown by glyph and weight.
 */
export function AnnotatedDiff({ file, lines, findings, agentLabel, onSend, emptyText }: AnnotatedDiffProps) {
  const [sends, setSends] = useState<Record<string, SendState>>({});
  const [openLine, setOpenLine] = useState<number | null>(null);
  const [draft, setDraft] = useState('');
  const [commentState, setCommentState] = useState<SendState | null>(null);
  const [sentLine, setSentLine] = useState<number | null>(null);

  const canSend = Boolean(agentLabel && onSend);
  const nums = newLineNumbers(lines);

  const headerFindings = findings.filter((f) => f.line === null);
  const byLine = new Map<number, ReviewFinding[]>();
  for (const f of findings) {
    if (f.line === null) continue;
    const list = byLine.get(f.line) ?? [];
    list.push(f);
    byLine.set(f.line, list);
  }

  const send = async (f: ReviewFinding) => {
    if (!onSend) return;
    setSends((s) => ({ ...s, [f.id]: { status: 'sending' } }));
    try {
      await onSend(f.file, f.line, f.text);
      setSends((s) => ({ ...s, [f.id]: { status: 'sent' } }));
    } catch (err) {
      setSends((s) => ({ ...s, [f.id]: { status: 'error', message: errText(err) } }));
    }
  };

  const sendComment = async (line: number) => {
    if (!onSend) return;
    const text = draft.trim();
    if (!text) return;
    setCommentState({ status: 'sending' });
    try {
      await onSend(file, line, text);
      setCommentState(null);
      setOpenLine(null);
      setDraft('');
      setSentLine(line);
    } catch (err) {
      setCommentState({ status: 'error', message: errText(err) });
    }
  };

  const toggleComment = (line: number) => {
    setCommentState(null);
    setSentLine(null);
    if (openLine === line) {
      setOpenLine(null);
      setDraft('');
    } else {
      setOpenLine(line);
      setDraft('');
    }
  };

  const renderFinding = (f: ReviewFinding) => {
    const state = sends[f.id];
    return (
      <span key={f.id} className="ck-find" role="note">
        <span className={`ck-find-sev ck-find-sev--${f.severity}`} role="img" aria-label={severityLabel(f.severity)}>
          {severityGlyph(f.severity)}
        </span>
        <span className="ck-find-text">{f.text}</span>
        {canSend && (
          <button
            type="button"
            className="ck-btn ck-btn--plain ck-btn--sm ck-find-send"
            disabled={state?.status === 'sending' || state?.status === 'sent'}
            onClick={() => void send(f)}
          >
            {state?.status === 'sent' ? 'Sent' : state?.status === 'sending' ? 'Sending…' : `Send to ${agentLabel}`}
          </button>
        )}
        {state?.status === 'error' && (
          <span className="ck-find-error" role="alert">
            {state.message}
          </span>
        )}
      </span>
    );
  };

  return (
    <pre className="ck-diff ck-diff-annot" aria-label={`Diff of ${file}`}>
      {headerFindings.map(renderFinding)}
      {lines.length === 0 && (
        <span className="ck-diff-line ck-diff-ctx">{emptyText ?? 'No textual changes.'}</span>
      )}
      {lines.map((line, i) => {
        const n = nums[i];
        const here = n !== null ? byLine.get(n) ?? [] : [];
        const top = here.reduce<Severity | null>(
          (best, f) => (best === null || RANK[f.severity] > RANK[best] ? f.severity : best),
          null,
        );
        const kind = lineKind(line);
        const commentable = canSend && n !== null && kind !== 'hunk';
        const composing = commentable && openLine === n;
        return (
          <Fragment key={i}>
            <span className={`ck-diff-line ck-diff-${kind}`}>
              {top ? (
                <span className="ck-gutter ck-gutter--mark" aria-hidden>
                  {severityGlyph(top)}
                </span>
              ) : (
                <span className="ck-gutter" aria-hidden />
              )}
              {line || ' '}
              {commentable && (
                <button
                  type="button"
                  className="ck-comment-btn"
                  aria-label={`Comment on line ${n}`}
                  aria-expanded={composing}
                  onClick={() => toggleComment(n as number)}
                >
                  <span aria-hidden>+</span>
                </button>
              )}
            </span>
            {here.map(renderFinding)}
            {composing && (
              <span className="ck-composer">
                <textarea
                  aria-label="Comment"
                  rows={2}
                  value={draft}
                  autoFocus
                  onChange={(e) => setDraft(e.target.value)}
                />
                <span className="ck-composer-actions">
                  {commentState?.status === 'error' && (
                    <span className="ck-find-error" role="alert">
                      {commentState.message}
                    </span>
                  )}
                  <button
                    type="button"
                    className="ck-btn ck-btn--secondary ck-btn--sm"
                    onClick={() => toggleComment(n as number)}
                  >
                    Cancel
                  </button>
                  <button
                    type="button"
                    className="ck-btn ck-btn--primary ck-btn--sm"
                    disabled={!draft.trim() || commentState?.status === 'sending'}
                    onClick={() => void sendComment(n as number)}
                  >
                    {`Send to ${agentLabel}`}
                  </button>
                </span>
              </span>
            )}
            {sentLine !== null && n !== null && sentLine === n && !composing && (
              <span className="ck-find ck-find-sent" role="status">
                Sent
              </span>
            )}
          </Fragment>
        );
      })}
    </pre>
  );
}
