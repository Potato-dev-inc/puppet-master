// The Needs you queue on the phone's Agents screen: one card per blocking item, in the PC's order
// (high risk and consent first, then oldest first). It is the PC's one attention list drawn as cards,
// so it can never show a different number than the counter above it. An approval or a folder-trust
// question is answered here (`ApprovalCard`); an `ask_user` question, a consent request, a spent
// budget and a usage limit keep their own cards (`QuestionsView`); anything else says what happened
// and offers its pane. Nothing is dropped: a card leaves only when the PC resolves it.
import { useCallback, useEffect, useState } from 'react';
import type { AttentionItem, AttentionSnapshot } from '../../lib/attention';
import { COPY } from '../../lib/attention-copy';
import type { BridgeClient } from '../../lib/bridge-types';
import { PROMPT_KINDS, QUESTION_KINDS, isOpen, subjectOf, visibleQueue } from '../../lib/needs-you-queue';
import type { Question } from '../../lib/questions';
import { useHeldItems } from '../../hooks/useHeldItems';
import { DotSpinner } from '../cockpit/DotSpinner';
import { ApprovalCard } from './ApprovalCard';
import { QuestionsView } from './QuestionsList';
import '../../styles/mobile-approval.css';

/** A card for an item with nothing to answer: an error, a stuck agent, a warning. */
export function NoteCard({ item, onOpenPane }: { item: AttentionItem; onOpenPane?: (paneId: string) => void }) {
  const paneId = item.pane?.id;
  return (
    <article className="ph-appr ph-appr--none ph-appr--note" data-item-kind={item.kind} data-card-state="note" aria-label={item.title}>
      <header className="ph-appr-head">
        <span className="ph-appr-glyph">
          <DotSpinner status={item.kind === 'error' ? 'error' : 'waiting_input'} />
        </span>
        <span className="ph-appr-pane">{item.pane?.heading ?? item.pane?.agent ?? 'Agent'}</span>
      </header>
      <div className="ph-appr-risk">
        <span className="ph-appr-title">{item.title}</span>
      </div>
      {item.sentence && <p className="ph-appr-status">{item.sentence}</p>}
      {paneId && onOpenPane && (
        <div className="ph-appr-actions">
          <button type="button" className="ph-btn ph-btn--plain" onClick={() => onOpenPane(paneId)}>
            {COPY.openPane}
          </button>
        </div>
      )}
    </article>
  );
}

export function NeedsYouList({
  snapshot,
  bridge,
  canAnswer,
  questions,
  refreshQuestions,
  labels,
  onOpenPane,
  showEmpty = false,
}: {
  snapshot: AttentionSnapshot;
  bridge: BridgeClient;
  canAnswer: boolean;
  /** The agents' pending questions (`GET /questions`): the choices and text of the question-like cards. */
  questions: Question[];
  refreshQuestions: () => void;
  labels: Map<string, string>;
  onOpenPane: (paneId: string) => void;
  /** Draw "Nothing needs you" when the queue is empty. */
  showEmpty?: boolean;
}) {
  const [now, setNow] = useState(() => Date.now());
  // Answers this phone gave (item id -> when): the tombstone of such a card leaves 3 s later.
  const [own, setOwn] = useState<ReadonlyMap<string, number>>(() => new Map());
  const markOwn = useCallback((id: string) => setOwn((prev) => (prev.has(id) ? prev : new Map(prev).set(id, Date.now()))), []);

  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 1_000);
    return () => window.clearInterval(timer);
  }, []);

  const queue = useHeldItems(visibleQueue(snapshot, own, now));
  const canControl = snapshot.device?.can_control ?? true;

  if (queue.length === 0) {
    return showEmpty ? (
      <div className="ph-queue-empty">
        <b>{COPY.empty.title}</b>
        {COPY.empty.body}
      </div>
    ) : null;
  }

  const byId = new Map(questions.map((q) => [q.id, q]));
  return (
    <section aria-label="Needs you" className="ph-queue" data-testid="needs-you-queue">
      {queue.map((item) => {
        if (PROMPT_KINDS.includes(item.kind)) {
          return (
            <ApprovalCard
              key={item.id}
              item={item}
              bridge={bridge}
              canAnswer={canAnswer}
              onOpenPane={onOpenPane}
              onAnswered={() => markOwn(item.id)}
            />
          );
        }
        if (QUESTION_KINDS.includes(item.kind) && isOpen(item)) {
          const q = byId.get(subjectOf(item));
          if (q) {
            // Budget and usage-limit cards spend or stop a run: without `control` the phone only reads them.
            const readOnly = !canAnswer ? COPY.watchOnly : (item.kind === 'budget' || item.kind === 'limit') && !canControl ? COPY.controlOnly : undefined;
            return (
              <div key={item.id} className="ph-qwrap" data-item-kind={item.kind}>
                <QuestionsView bridge={bridge} questions={[q]} refresh={refreshQuestions} labels={labels} bare readOnly={readOnly} />
              </div>
            );
          }
        }
        return <NoteCard key={item.id} item={item} onOpenPane={onOpenPane} />;
      })}
    </section>
  );
}
