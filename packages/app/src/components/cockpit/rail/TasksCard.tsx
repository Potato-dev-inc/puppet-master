// The agent's current plan: one line per task with a dot for pending, working or done. Hidden with no plan.
import type { PlanItem, PlanStatus } from '../../../lib/sessionChatModel';
import { RailCard } from './RailCard';

const STATE_LABEL: Record<PlanStatus, string> = { pending: 'To do', in_progress: 'Working on it', completed: 'Done' };

export function TasksCard({ plan }: { plan: readonly PlanItem[] }) {
  if (plan.length === 0) return null;
  const done = plan.filter((t) => t.status === 'completed').length;
  return (
    <RailCard title="Tasks" note={`${done} of ${plan.length} done`} testId="rail-tasks">
      <ul className="ck-rail-list">
        {plan.map((task, i) => (
          <li key={task.taskId ?? i} className={`ck-rail-task is-${task.status}`}>
            <span className="ck-rail-dot" role="img" aria-label={STATE_LABEL[task.status]} />
            <span className="ck-rail-task-text" title={task.text}>
              {task.text}
            </span>
          </li>
        ))}
      </ul>
    </RailCard>
  );
}
