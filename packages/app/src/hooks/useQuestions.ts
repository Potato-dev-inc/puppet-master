// Pending `ask_user` questions for the cockpit: loaded once, refreshed on `questions://changed`,
// with a slow poll as a safety net (events are fire-and-forget).
import { useCallback, useEffect, useState } from 'react';
import { cockpitApi } from '../lib/cockpit-api';
import { isLive, type Question, type QuestionAnswer } from '../lib/questions';

const POLL_MS = 15000;

export interface QuestionsApi {
  /** Live questions, oldest first. */
  questions: Question[];
  answerQuestion: (id: string, answer: QuestionAnswer) => Promise<void>;
  dismissQuestion: (id: string) => Promise<void>;
}

export function useQuestions(notify: (text: string) => void, now: number): QuestionsApi {
  const [all, setAll] = useState<Question[]>([]);

  const refresh = useCallback(() => {
    cockpitApi
      .questionsList()
      .then(setAll)
      .catch(() => {});
  }, []);

  useEffect(() => {
    let disposed = false;
    let off: (() => void) | undefined;
    refresh();
    void cockpitApi.onQuestionsChanged(refresh).then((u) => (disposed ? u() : (off = u)));
    const timer = window.setInterval(refresh, POLL_MS);
    return () => {
      disposed = true;
      off?.();
      window.clearInterval(timer);
    };
  }, [refresh]);

  const settle = useCallback(
    async (id: string, action: Promise<{ accepted: boolean }>, done: string) => {
      try {
        const res = await action;
        notify(res.accepted ? done : 'Already resolved');
      } catch (err) {
        notify(err instanceof Error ? err.message : String(err));
      }
      setAll((cur) => cur.filter((q) => q.id !== id));
      refresh();
    },
    [notify, refresh],
  );

  const answerQuestion = useCallback(
    (id: string, answer: QuestionAnswer) =>
      settle(id, cockpitApi.answerQuestion(id, 'choice_id' in answer ? { choice_id: answer.choice_id } : { text: answer.text }), 'Answer sent'),
    [settle],
  );
  const dismissQuestion = useCallback((id: string) => settle(id, cockpitApi.dismissQuestion(id), 'Dismissed'), [settle]);

  return { questions: all.filter((q) => isLive(q, now)), answerQuestion, dismissQuestion };
}
