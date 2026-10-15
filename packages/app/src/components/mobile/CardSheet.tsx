// A full-screen card for a push tap on an item that has no pane to open (an agent's question, a consent
// request), or on one the list no longer holds. It draws the same cards as the Needs you list.
import { useEffect, useState } from 'react';
import type { AttentionItem, AttentionSnapshot } from '../../lib/attention';
import { COPY } from '../../lib/attention-copy';
import type { BridgeClient } from '../../lib/bridge-types';
import { PROMPT_KINDS, QUESTION_KINDS, isOpen, subjectOf } from '../../lib/needs-you-queue';
import type { Question } from '../../lib/questions';
import { ApprovalCard } from './ApprovalCard';
import { Sheet } from './MobileSheet';
import { NoteCard } from './NeedsYouList';
import { QuestionsView } from './QuestionsList';

export function CardSheet({
  item,
  snapshot,
  bridge,
  canAnswer,
  labels,
  onClose,
  onOpenPane,
}: {
  item: AttentionItem;
  snapshot: AttentionSnapshot;
  bridge: BridgeClient;
  canAnswer: boolean;
  labels: Map<string, string>;
  onClose: () => void;
  onOpenPane: (paneId: string) => void;
}) {
  const [questions, setQuestions] = useState<Question[]>([]);
  const isQuestion = QUESTION_KINDS.includes(item.kind);
  const refresh = () => {
    bridge
      .listQuestions()
      .then(setQuestions)
      .catch(() => {});
  };
  useEffect(() => {
    if (isQuestion) refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [item.id]);

  // The item as the list holds it now (a card opened from a tap keeps up with the PC).
  const live = snapshot.items.find((i) => i.id === item.id) ?? item;
  const question = isQuestion ? questions.find((q) => q.id === subjectOf(live)) : undefined;
  return (
    <Sheet title={live.pane?.heading ?? 'Needs you'} onClose={onClose}>
      <div className="ph-sheet-card" data-testid="card-sheet">
        {PROMPT_KINDS.includes(live.kind) && (
          <ApprovalCard item={live} bridge={bridge} canAnswer={canAnswer} onOpenPane={onOpenPane} autoFocus />
        )}
        {isQuestion && isOpen(live) && question && (
          <QuestionsView bridge={bridge} questions={[question]} refresh={refresh} labels={labels} bare readOnly={canAnswer ? undefined : COPY.watchOnly} />
        )}
        {isQuestion && !isOpen(live) && <NoteCard item={{ ...live, sentence: live.resolution?.state === 'answered' ? `Answered${live.resolution.choice_label ? `: ${live.resolution.choice_label}` : ''}.` : COPY.dealtWith }} onOpenPane={onOpenPane} />}
        {!PROMPT_KINDS.includes(live.kind) && !isQuestion && <NoteCard item={live} onOpenPane={onOpenPane} />}
      </div>
    </Sheet>
  );
}
