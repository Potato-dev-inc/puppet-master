// The pull request made from a separate copy: its link, the GitHub checks (polled a bounded number
// of times), and, when a check failed, "Send back to <agent>". Sending asks first and shows exactly
// what the agent will receive; nothing is ever sent automatically.
import { useEffect, useRef, useState } from 'react';
import { shipApi, type PrRef, type SendBackPreview, type ShipApi } from '../../lib/ship-api';
import { checksSentence } from '../../lib/ship-model';
import { DotSpinner } from './DotSpinner';
import { Sheet } from './Sheet';
import '../../styles/cockpit-ship.css';

const errText = (err: unknown): string => (err instanceof Error ? err.message : String(err));

/** Looks at the checks at most this many times per mount; each look may wait on the backend. */
export const MAX_POLLS = 30;

export function PrBlock({
  repo,
  handle,
  pr,
  agentName = 'the agent',
  ship = shipApi,
  pollEveryMs = 4000,
  waitSecs = 20,
  onPr,
}: {
  repo: string;
  handle: string;
  pr: PrRef;
  /** Display name of the agent that wrote the changes. */
  agentName?: string;
  ship?: ShipApi;
  pollEveryMs?: number;
  waitSecs?: number;
  onPr: (pr: PrRef) => void;
}) {
  const [problem, setProblem] = useState<string | null>(null);
  const [gaveUp, setGaveUp] = useState(false);
  const [round, setRound] = useState(0);
  const [preview, setPreview] = useState<SendBackPreview | null>(null);
  const [sending, setSending] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const onPrRef = useRef(onPr);
  onPrRef.current = onPr;
  const state = pr.checks?.state;

  // Bounded polling: one look now, then again while something is pending, never more than MAX_POLLS.
  useEffect(() => {
    if (state && state !== 'pending' && round === 0) return;
    let stop = false;
    setGaveUp(false);
    const loop = async () => {
      for (let n = 0; n < MAX_POLLS && !stop; n++) {
        try {
          const next = await ship.prChecks(repo, handle, n === 0 ? 0 : waitSecs);
          if (stop) return;
          setProblem(null);
          onPrRef.current(next);
          if (next.checks && next.checks.state !== 'pending') return;
        } catch (err) {
          if (!stop) setProblem(errText(err));
          return;
        }
        await new Promise((resolve) => setTimeout(resolve, pollEveryMs));
      }
      if (!stop) setGaveUp(true);
    };
    void loop();
    return () => {
      stop = true;
    };
    // `state` is deliberately not a dependency: the loop owns it. A new `round` starts a new loop.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ship, repo, handle, round]);

  const failing = pr.checks?.state === 'failing';
  const openOnGitHub = () => void ship.openUrl(pr.url);

  const askSendBack = async () => {
    setProblem(null);
    try {
      setPreview(await ship.prSendBackPreview(repo, handle));
    } catch (err) {
      setProblem(errText(err));
    }
  };

  const send = async () => {
    if (!preview) return;
    setSending(true);
    try {
      await ship.prSendBack(repo, handle, preview.text);
      setNotice(`Sent to ${agentName}. Check again when it has pushed a fix.`);
      setPreview(null);
    } catch (err) {
      setProblem(errText(err));
      setPreview(null);
    } finally {
      setSending(false);
    }
  };

  const glyph = !pr.checks || pr.checks.state === 'pending' ? 'running' : failing ? 'error' : 'idle';

  return (
    <section className="ck-pr" aria-label="Pull request">
      <div className="ck-pr-head">
        <span className="ck-pr-title">
          {pr.draft ? 'Draft PR' : 'PR'} #{pr.number}
        </span>
        <button type="button" className="ck-link" onClick={openOnGitHub}>
          Open on GitHub
        </button>
      </div>
      <p className="ck-pr-checks" role="status">
        <span className="ck-pr-glyph" aria-hidden="true">
          <DotSpinner status={glyph} />
        </span>
        <span>{problem ? 'Could not read the GitHub checks.' : checksSentence(pr.checks)}</span>
      </p>
      {problem && <p className="ck-changes-error" role="alert">{problem}</p>}
      {gaveUp && !problem && <p className="ck-sub">Still running on GitHub. Check again later.</p>}
      {notice && <p className="ck-sub">{notice}</p>}
      <div className="ck-pr-actions">
        {failing && (
          <button type="button" className="ck-btn ck-btn--primary" onClick={() => void askSendBack()}>
            Send back to {agentName}
          </button>
        )}
        {(pr.checks?.state !== 'pending' || gaveUp) && (
          <button type="button" className="ck-btn ck-btn--secondary" onClick={() => setRound((r) => r + 1)}>
            Check again
          </button>
        )}
      </div>

      {preview && (
        <Sheet labelledBy="ck-sendback-t" role="dialog" onClose={sending ? undefined : () => setPreview(null)}>
          <h2 className="ck-section" id="ck-sendback-t">
            Send back to {agentName}?
          </h2>
          <p className="ck-sub">
            {agentName} gets the failing job&apos;s last {preview.excerpt_lines > 0 ? preview.excerpt_lines : 'few'} lines, marked as output
            from GitHub. Failing: {preview.failing.join(', ')}.
          </p>
          <details className="ck-pr-details">
            <summary>See exactly what is sent</summary>
            <pre className="ck-pr-command">{preview.text}</pre>
          </details>
          <div className="ck-sheet-actions">
            <button type="button" className="ck-btn ck-btn--secondary" autoFocus disabled={sending} onClick={() => setPreview(null)}>
              Cancel
            </button>
            <button type="button" className="ck-btn ck-btn--primary" disabled={sending} onClick={() => void send()}>
              Send
            </button>
          </div>
        </Sheet>
      )}
    </section>
  );
}
