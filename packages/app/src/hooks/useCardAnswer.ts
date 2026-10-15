// One approval card's answer: the tap, the request, and what comes back. A tap makes an answer key
// once; "Try again" sends the SAME key, so a retry over a flaky tunnel can never press twice (the PC
// returns the first result). Nothing is stored for later: an approval must hit a live screen, so a tap
// with no way to the PC fails at once and says so (design 2.2 point 5).
import { useCallback, useEffect, useRef, useState } from 'react';
import { AttentionAnswerError, type AttentionAnswerOutcome, type AttentionItem } from '../lib/attention';
import { SENDING_PHASE_MS } from '../lib/attention-copy';
import type { BridgeClient } from '../lib/bridge-types';
import { IDLE, type LocalAnswer } from '../lib/approval-card';

/** How often the card re-reads the clock while an answer is on its way. */
const TICK_MS = 500;

export function newAnswerKey(): string {
  const c = globalThis.crypto as Crypto | undefined;
  if (c?.randomUUID) return c.randomUUID();
  return `k${Date.now().toString(36)}${Math.random().toString(36).slice(2, 10)}`;
}

/** Turn what the PC said into the card's own state. */
export function localFromOutcome(outcome: AttentionAnswerOutcome, sent: { choiceId: string; label: string }, now: number): LocalAnswer {
  switch (outcome.outcome) {
    case 'answered':
      return { phase: 'answered', label: outcome.choice_label || sent.label, at: outcome.at_ms || now };
    case 'already_answered':
      return { phase: 'already', by: outcome.by, deviceName: outcome.device_name ?? undefined, label: outcome.choice_label ?? undefined };
    case 'changed':
      return { phase: 'changed' };
    case 'gone':
      return { phase: 'gone' };
    default:
      return { phase: 'failed', why: 'server', choiceId: sent.choiceId, label: sent.label };
  }
}

export interface CardAnswer {
  local: LocalAnswer;
  /** The clock the card reads (it ticks while an answer is on its way). */
  now: number;
  send: (choiceId: string) => void;
  retry: () => void;
  /** Forget a refusal and draw the card again with what the PC lists now. */
  reset: () => void;
}

export function useCardAnswer(
  bridge: Pick<BridgeClient, 'answerAttention'> | null,
  item: AttentionItem,
  /** The PC said something other than "answered" (changed, gone, already answered, not sent): read the list again now. */
  onStale?: () => void,
): CardAnswer {
  const [local, setLocal] = useState<LocalAnswer>(IDLE);
  const [now, setNow] = useState(() => Date.now());
  const keyRef = useRef<string | null>(null);
  const inflight = useRef<AbortController | null>(null);
  const alive = useRef(true);
  const itemRef = useRef(item);
  itemRef.current = item;
  const staleRef = useRef(onStale);
  staleRef.current = onStale;

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      inflight.current?.abort();
    };
  }, []);

  // Re-read the clock while an answer is on its way, so "Sending" turns to "Still sending".
  const sending = local.phase === 'sending';
  useEffect(() => {
    if (!sending) return;
    setNow(Date.now());
    const timer = window.setInterval(() => setNow(Date.now()), TICK_MS);
    return () => window.clearInterval(timer);
  }, [sending]);

  const run = useCallback(
    (choiceId: string, label: string, key: string) => {
      const current = itemRef.current;
      const prompt = current.prompt;
      if (!bridge?.answerAttention || !prompt) return;
      inflight.current?.abort();
      const controller = new AbortController();
      inflight.current = controller;
      const startedAt = Date.now();
      setNow(startedAt);
      setLocal({ phase: 'sending', choiceId, label, startedAt });
      const giveUp = window.setTimeout(() => controller.abort(), SENDING_PHASE_MS.lost);
      void bridge
        .answerAttention(
          current.id,
          { choice: choiceId, prompt_id: prompt.prompt_id, occurrence: prompt.occurrence, fingerprint: prompt.fingerprint, answer_key: key },
          controller.signal,
        )
        .then(
          (outcome) => {
            if (!alive.current || inflight.current !== controller) return;
            setLocal(localFromOutcome(outcome, { choiceId, label }, Date.now()));
            if (outcome.outcome !== 'answered') staleRef.current?.();
          },
          (error: unknown) => {
            if (!alive.current || inflight.current !== controller) return;
            if (error instanceof AttentionAnswerError) {
              setLocal({ phase: 'failed', why: 'server', choiceId, label });
            } else if (controller.signal.aborted) {
              setLocal({ phase: 'failed', why: 'lost', choiceId, label });
            } else {
              // The request never got to the PC (no network, the tunnel is down).
              setLocal({ phase: 'failed', why: 'offline', choiceId, label });
            }
          },
        )
        .finally(() => window.clearTimeout(giveUp));
    },
    [bridge],
  );

  const send = useCallback(
    (choiceId: string) => {
      const choice = itemRef.current.prompt?.choices?.find((c) => c.id === choiceId);
      if (!choice) return;
      // A tap while an answer is already on its way is the same tap again, not a second answer.
      if (local.phase === 'sending') return;
      keyRef.current = newAnswerKey();
      run(choiceId, choice.label, keyRef.current);
    },
    [run, local],
  );

  const retry = useCallback(() => {
    if (local.phase !== 'failed' || !keyRef.current) return;
    run(local.choiceId, local.label, keyRef.current);
  }, [run, local]);

  const reset = useCallback(() => {
    inflight.current?.abort();
    inflight.current = null;
    keyRef.current = null;
    setLocal(IDLE);
  }, []);

  return { local, now, send, retry, reset };
}
