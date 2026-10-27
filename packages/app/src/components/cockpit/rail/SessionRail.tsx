// The "Session" rail beside a pane's main area: small cards that say where the pane runs, how full its context
// is, what the agent plans, what it changed and which helpers it started. It reads the same session file as the
// Chat tab (the backend shares one reader between the views; this one holds it under its own id).
import '../../../styles/cockpit-rail.css';
import { useSessionChat } from '../../../hooks/useSessionChat';
import { ChangesCard } from './ChangesCard';
import { ContextCard } from './ContextCard';
import { HelpersCard, type HelperRow } from './HelpersCard';
import { TasksCard } from './TasksCard';
import { WhereCard } from './WhereCard';
import { usePaneBranch } from './usePaneBranch';

export interface SessionRailProps {
  paneId: string;
  status: string;
  cwd: string;
  agentName: string;
  accountLabel?: string | null;
  /** Read the agent's session file (false for a plain shell: it has none). */
  readsChat: boolean;
  /** "Helper of X" and the pane it opens, when this pane was started by another. */
  helperOf: { parentId: string; text: string } | null;
  helpers: readonly HelperRow[];
  /** Set when the pane has a Changes tab; a file in the Changes card opens it. */
  onOpenChanges?: () => void;
  onOpenPane: (id: string) => void;
}

export function SessionRail({ paneId, status, cwd, agentName, accountLabel, readsChat, helperOf, helpers, onOpenChanges, onOpenPane }: SessionRailProps) {
  const chat = useSessionChat(paneId, readsChat);
  const branch = usePaneBranch(paneId, status, true);
  return (
    <aside className="ck-rail" aria-label="Session details" data-testid="session-rail">
      {helperOf && (
        <button type="button" className="ck-rail-parent" onClick={() => onOpenPane(helperOf.parentId)} title="Open the pane that started this one">
          {helperOf.text}
        </button>
      )}
      <WhereCard agentName={agentName} model={chat.model} cliVersion={chat.cliVersion} folder={cwd} branch={branch} accountLabel={accountLabel} />
      <ContextCard usage={chat.usage} />
      <TasksCard plan={chat.plan} />
      <ChangesCard outputs={chat.outputs} cwd={cwd} onOpenChanges={onOpenChanges} />
      <HelpersCard helpers={helpers} onOpen={onOpenPane} />
    </aside>
  );
}
