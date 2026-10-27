// The agent's task list, pinned at the top of the chat: "Tasks 2/5", then each task with a dot glyph (empty:
// to do, ringed with a dot: in progress, filled with a tick: done). Shows only; nothing here is clickable.
import type { ChatTurn, PlanItem, PlanStatus } from '../../lib/sessionChatModel';

/** The task list as it stands now: the list after the newest turn that updated it. Empty when there is none. */
export function currentPlan(turns: readonly ChatTurn[]): PlanItem[] {
  for (let i = turns.length - 1; i >= 0; i--) {
    const plan = turns[i]!.plan;
    if (plan) return plan;
  }
  return [];
}

const GLYPH_LABEL: Record<PlanStatus, string> = {
  pending: 'To do',
  in_progress: 'In progress',
  completed: 'Done',
};

export function PlanCard({ items }: { items: readonly PlanItem[] }) {
  if (items.length === 0) return null;
  const done = items.filter((item) => item.status === 'completed').length;
  return (
    <section aria-label="Tasks" className="shrink-0 border-b border-pm-border bg-pm-panel px-2 py-1.5 ck-chat-plan" data-testid="chat-plan">
      <h3 className="text-[11px] font-semibold text-pm-muted ck-chat-plan-head">
        Tasks {done}/{items.length}
      </h3>
      <ol className="mt-1 max-h-32 space-y-0.5 overflow-y-auto ck-chat-plan-list">
        {items.map((item, index) => (
          <li
            key={item.taskId ?? `${index}`}
            data-status={item.status}
            className={`flex min-w-0 items-start gap-2 text-[12px] ck-chat-plan-item ck-chat-plan-item--${item.status}`}
          >
            <span role="img" aria-label={GLYPH_LABEL[item.status]} className={`mt-[5px] flex h-2.5 w-2.5 shrink-0 items-center justify-center rounded-full ck-chat-plan-glyph ck-chat-plan-glyph--${item.status}`} />
            <span className={`min-w-0 break-words ${item.status === 'in_progress' ? 'text-pm-text' : ''}`}>{item.text}</span>
          </li>
        ))}
      </ol>
    </section>
  );
}
