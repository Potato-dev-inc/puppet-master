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
    <section aria-label="Questions from agents" className="mb-2 flex flex-col gap-2">
      {live.map((q) => (
        <div key={q.id} className="rounded-lg border border-pm-warn/60 bg-pm-panel p-3" data-question-id={q.id}>
          <div className="text-[11px] text-pm-muted">
            Question from {askerName(q, q.pane_id ? labels.get(q.pane_id) : undefined)}
          </div>
          <p className="mt-1 whitespace-pre-wrap break-words text-sm font-medium text-zinc-100">{q.question}</p>
          {q.context && <p className="mt-1 whitespace-pre-wrap break-words text-xs text-pm-muted">{q.context}</p>}
          {q.choices.length > 0 && (
            <div className="mt-2 flex flex-wrap gap-2">
              {q.choices.map((c, i) => (
                <button
                  key={c.id}
                  type="button"
                  onClick={() => void run(bridge.answerQuestion(q.id, { choice_id: c.id }))}
                  className={`min-h-[44px] rounded px-3 text-xs ${
                    i === 0 ? 'bg-zinc-100 text-zinc-900' : 'border border-pm-border text-zinc-100 active:bg-pm-raised'
                  }`}
                >
                  {c.label}
                </button>
              ))}
            </div>
          )}
          {q.allow_free_text && (
            <form
              className="mt-2 flex gap-2"
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
                className="min-h-[44px] min-w-0 flex-1 rounded border border-pm-border bg-pm-bg px-2 text-base text-zinc-100"
              />
              <button
                type="submit"
                className="min-h-[44px] rounded border border-pm-border px-3 text-xs text-zinc-100 active:bg-pm-raised"
              >
                Send
              </button>
            </form>
          )}
          <button
            type="button"
            onClick={() => void run(bridge.dismissQuestion(q.id))}
            className="mt-2 min-h-[44px] text-xs text-pm-muted active:text-zinc-100"
          >
            Dismiss
          </button>
        </div>
      ))}
      {error && <div className="text-xs text-pm-err">{error}</div>}
    </section>
  );
}
