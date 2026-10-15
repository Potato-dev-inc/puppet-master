// Pending `ask_user` questions at the top of the phone All tab: the question, choice buttons and
// a text field. Refetches when the bridge `questions` event changes `revision` (the event-driven
// path), fetches once on mount, and keeps a slow 30s poll only as a fallback for a missed event.
import { useCallback, useEffect, useRef, useState } from 'react';
import type { BridgeClient } from '../../lib/bridge';
import { askerName, isLive, waitCaption, type Question } from '../../lib/questions';

/** Fallback poll: the `questions` SSE event is the primary trigger, this only catches missed events. */
const POLL_MS = 30000;

export function useQuestionPolling(bridge: BridgeClient, bridgeReady: boolean, revision = 0) {
  const [questions, setQuestions] = useState<Question[]>([]);
  const refresh = useCallback(() => {
    bridge
      .listQuestions()
      .then(setQuestions)
      .catch(() => {});
  }, [bridge]);
  useEffect(() => {
    if (!bridgeReady) return;
    refresh();
    const timer = window.setInterval(refresh, POLL_MS);
    return () => window.clearInterval(timer);
  }, [bridgeReady, refresh]);
  // Refetch on a revision change only; the mount fetch above already covers the initial value.
  const seenRevision = useRef(revision);
  useEffect(() => {
    if (revision === seenRevision.current) return;
    seenRevision.current = revision;
    if (bridgeReady) refresh();
  }, [bridgeReady, revision, refresh]);
  return { questions, refresh };
}

export function QuestionsList({
  bridge,
  bridgeReady,
  labels,
  revision = 0,
}: {
  bridge: BridgeClient;
  bridgeReady: boolean;
  labels: Map<string, string>;
  /** Bumped by the bridge `questions` event; a change refetches the list. */
  revision?: number;
}) {
  const { questions, refresh } = useQuestionPolling(bridge, bridgeReady, revision);
  return <QuestionsView bridge={bridge} questions={questions} refresh={refresh} labels={labels} />;
}

/** The cards for questions someone else fetched (PwaApp also counts them in the waiting number). */
export function QuestionsView({
  bridge,
  questions,
  refresh,
  labels,
  bare = false,
  readOnly,
}: {
  bridge: BridgeClient;
  questions: Question[];
  refresh: () => void;
  labels: Map<string, string>;
  /** Draw the cards without their own section, for a list that lays them out (the Needs you queue). */
  bare?: boolean;
  /** Why this phone may not answer (shown in place of the choices and the text box). */
  readOnly?: string;
}) {
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [error, setError] = useState<string | null>(null);
  // Re-render twice a minute so the "stops waiting in N min" caption keeps counting down.
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 30_000);
    return () => window.clearInterval(timer);
  }, []);
  const live = questions.filter((q) => isLive(q, now));
  if (live.length === 0) return null;

  const run = async (action: Promise<unknown>) => {
    try {
      await action;
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
    refresh();
  };

  const cards = (
    <>
      {live.map((q) => (
        <div key={q.id} className="ph-card" data-question-id={q.id}>
          <div className="ph-card-cap">
            {q.limit ? `${q.limit.agent_label} · usage limit` : `Question from ${askerName(q, q.pane_id ? labels.get(q.pane_id) : undefined)}`}
          </div>
          <p className="ph-card-q">{q.question}</p>
          {q.context && <p className="ph-card-body">{q.context}</p>}
          {!readOnly && q.choices.length > 0 && (
            <div className="ph-answers">
              {/* Equal weight on purpose: the agent (or the limit card) wrote the choices, so none is made the easy default. */}
              {q.choices.map((c) => (
                <button
                  key={c.id}
                  type="button"
                  onClick={() => void run(bridge.answerQuestion(q.id, { choice_id: c.id }))}
                  className="ph-btn"
                >
                  {c.label}
                </button>
              ))}
            </div>
          )}
          {readOnly && <p className="ph-card-wait">{readOnly}</p>}
          {/* A usage-limit card does not time out on an asking agent: it goes away with the limit. */}
          {!q.limit && (
            <p className="ph-card-wait" data-testid="question-wait">
              {waitCaption(q, now, q.pane_id ? labels.get(q.pane_id) : undefined)}
            </p>
          )}
          {!readOnly && q.allow_free_text && (
            <form
              className="ph-reply"
              onSubmit={(e) => {
                e.preventDefault();
                const text = (drafts[q.id] ?? '').trim();
                if (!text) return;
                setDrafts((d) => ({ ...d, [q.id]: '' }));
                void run(bridge.answerQuestion(q.id, { text }));
              }}
            >
              <input
                value={drafts[q.id] ?? ''}
                onChange={(e) => setDrafts((d) => ({ ...d, [q.id]: e.target.value }))}
                placeholder={q.choices.length > 0 ? 'Or type an answer' : 'Type an answer'}
                aria-label="Your answer"
                maxLength={4000}
                className="ph-field"
              />
              <button type="submit" className="ph-btn">
                Send
              </button>
            </form>
          )}
          {/* A limit card's own choices end it ("Stop here" / "Dismiss"); a second Dismiss would leave the run parked. */}
          {!readOnly && !q.limit && (
            <button
              type="button"
              onClick={() => void run(bridge.dismissQuestion(q.id))}
              className="ph-btn ph-btn--plain"
              style={{ marginTop: 4 }}
            >
              Dismiss
            </button>
          )}
        </div>
      ))}
      {error && <div className="ph-error">{error}</div>}
    </>
  );
  if (bare) return cards;
  return (
    <section aria-label="Questions from agents" className="ph-cards">
      {cards}
    </section>
  );
}
