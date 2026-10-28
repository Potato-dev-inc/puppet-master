// What viewers see of the chat, read-only, as plain text. The Go live sheet shows the exact first snapshot
// with this, so the owner checks it before anything is shared. Every value is text: React escapes it, so a
// `<b>` written in a chat stays text.
import type { LiveItem, LivePane, LiveTask } from '../../../lib/live-room';
import '../../../styles/cockpit-live.css';

const WHO: Record<LiveItem['t'], string> = {
  user: 'Prompt',
  agent: 'Reply',
  tool: 'Step',
  result: 'Result',
  end: 'Turn',
  plan: 'Plan',
  note: 'Note',
};

function taskLine(task: LiveTask): string {
  return task.status ? `${task.text} (${task.status})` : task.text;
}

function itemBody(item: LiveItem): string {
  switch (item.t) {
    case 'user':
    case 'agent':
    case 'note':
      return item.text;
    case 'tool': {
      const files = item.files === 1 ? '1 file' : `${item.files} files`;
      const parts = [item.name, item.summary, item.files > 0 ? files : '', item.writes ? 'writes' : ''];
      return parts.filter(Boolean).join(' · ');
    }
    case 'result':
      const lines = item.lines === 1 ? '1 line' : `${item.lines} lines`;
      return `${item.ok ? 'Done' : 'Failed'} · ${lines}`;
    case 'end': {
      const took = item.ms === null ? '' : ` · ${Math.round(item.ms / 1000)} s`;
      return `${item.outcome}${took}`;
    }
    case 'plan':
      return item.replace ? 'Plan updated' : 'Plan';
  }
}

export function LivePreview({
  items,
  hidden = 0,
  panes = [],
}: {
  items: LiveItem[];
  /** Steps that were read but not shared (paused, or screen chat while a card was open). */
  hidden?: number;
  /** Pane labels, to show which agent a step belongs to. */
  panes?: LivePane[];
}) {
  if (items.length === 0 && hidden === 0) {
    return <p className="ck-live-empty">Nothing to show yet. The chat appears here when an agent answers.</p>;
  }
  return (
    <>
      <ol className="ck-live-preview" aria-label="What viewers see">
        {items.map((item) => {
          const label = panes.find((p) => p.key === item.pane)?.label;
          return (
            <li key={`${item.pane}-${item.id}`} className={`ck-live-item is-${item.t}`}>
              <span className="ck-live-who">{WHO[item.t]}</span>
              {label && <span className="ck-live-src">{label}</span>}
              {item.t === 'plan' ? (
                <>
                  <span className="ck-live-body">{itemBody(item)}</span>
                  <ul className="ck-live-tasks">
                    {item.tasks.map((task, i) => (
                      <li key={i}>{taskLine(task)}</li>
                    ))}
                  </ul>
                </>
              ) : (
                <span className="ck-live-body">{itemBody(item)}</span>
              )}
            </li>
          );
        })}
      </ol>
      {hidden > 0 && <p className="ck-live-hidden">Some steps were not shared</p>}
    </>
  );
}
