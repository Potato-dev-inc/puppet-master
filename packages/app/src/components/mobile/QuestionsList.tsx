// Pending `ask_user` questions at the top of the phone All tab: the question, choice buttons and
// a text field. Polls the bridge while mounted (the SSE `questions` event is a hint, not required).
import { useCallback, useEffect, useState } from 'react';
import type { BridgeClient } from '../../lib/bridge';
import { askerName, isLive, type Question } from '../../lib/questions';

const POLL_MS = 3000;

export function useQuestionPolling(bridge: BridgeClient, bridgeReady: boolean) {
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
  return { questions, refresh };
}

export function QuestionsList({
  bridge,
  bridgeReady,
  labels,
}: {
  bridge: BridgeClient;
  bridgeReady: boolean;
  labels: Map<string, string>;
}) {
  const { questions, refresh } = useQuestionPolling(bridge, bridgeReady);
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [error, setError] = useState<string | null>(null);
  const live = questions.filter((q) => isLive(q, Date.now()));
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

  return (
    <section aria-label="Questions from agents" className="ph-cards">
      {live.map((q) => (
        <div key={q.id} className="ph-card" data-question-id={q.id}>
          <div className="ph-card-cap">
            Question from {askerName(q, q.pane_id ? labels.get(q.pane_id) : undefined)}
          </div>
          <p className="ph-card-q">{q.question}</p>
          {q.context && <p className="ph-card-body">{q.context}</p>}
          {q.choices.length > 0 && (
            <div className="ph-answers">
              {q.choices.map((c, i) => (
                <button
                  key={c.id}
                  type="button"
                  onClick={() => void run(bridge.answerQuestion(q.id, { choice_id: c.id }))}
                  className={i === 0 ? 'ph-btn ph-btn--primary' : 'ph-btn'}
                >
                  {c.label}
                </button>
              ))}
            </div>
          )}
          {q.allow_free_text && (
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
          <button
            type="button"
            onClick={() => void run(bridge.dismissQuestion(q.id))}
            className="ph-btn ph-btn--plain"
            style={{ marginTop: 4 }}
          >
            Dismiss
          </button>
        </div>
      ))}
      {error && <div className="ph-error">{error}</div>}
    </section>
  );
}
