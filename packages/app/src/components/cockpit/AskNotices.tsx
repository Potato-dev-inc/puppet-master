import { tauriAskbarApi, useAskNotices, type AskbarApi, type AskNotice } from '../../lib/askbar';
import '../../styles/ask-notices.css';

const PREVIEW_CHARS = 160;

/** The text under the question: the answer in one short line, or the error when it failed. */
export function noticePreview(notice: AskNotice): string {
  const raw = notice.result.ok ? (notice.result.answer ?? '') : (notice.result.error?.message ?? 'The agent could not answer.');
  const oneLine = raw.replace(/\s+/g, ' ').trim();
  return oneLine.length > PREVIEW_CHARS ? `${oneLine.slice(0, PREVIEW_CHARS)}…` : oneLine;
}

/** Answers from the Ask bar that arrived while the bar was hidden. Renders nothing when there are none. */
export function AskNotices({ api = tauriAskbarApi }: { api?: AskbarApi }) {
  const { notices, dismiss, open } = useAskNotices(api);
  if (notices.length === 0) return null;
  return (
    <ul className="ck-ask-list" aria-label="Answers from your agents">
      {notices.map((n) => (
        <li key={n.id} className="ck-ask">
          <p className="ck-ask-title">Answer from {n.agent_label}</p>
          <p className="ck-ask-q">{n.question}</p>
          <p className="ck-ask-a">{noticePreview(n)}</p>
          <div className="ck-ask-actions">
            <button type="button" className="ck-btn ck-btn--sm ck-btn--primary" onClick={() => open(n.id)}>
              Open
            </button>
            <button type="button" className="ck-btn ck-btn--sm ck-btn--secondary" onClick={() => dismiss(n.id)}>
              Dismiss
            </button>
          </div>
        </li>
      ))}
    </ul>
  );
}
