import { useCallback, useEffect, useMemo, useState } from 'react';
import type {
  AuditEntryProjection,
  BridgeClient,
  ContextPack,
  LockProjection,
  TaskProjection,
  WorkspaceStateProjection,
} from '../lib/bridge';
import type { AgentContextProfile } from '@puppet-master/shared';

type TabKey = 'overview' | 'tasks' | 'locks' | 'audit' | 'context' | 'agents';

interface Props {
  bridge: BridgeClient | null;
  bridgeReady: boolean;
}

interface CoordinationSnapshot {
  workspace: WorkspaceStateProjection | null;
  tasks: TaskProjection[];
  locks: LockProjection[];
  audit: AuditEntryProjection[];
  agents: AgentContextProfile[];
}

const emptySnapshot: CoordinationSnapshot = {
  workspace: null,
  tasks: [],
  locks: [],
  audit: [],
  agents: [],
};

function shortId(id: string): string {
  if (id.length <= 10) return id;
  return `${id.slice(0, 4)}…${id.slice(-4)}`;
}

function relativeTime(timestampMs: number): string {
  const delta = Date.now() - timestampMs;
  if (delta < 1000) return 'now';
  if (delta < 60_000) return `${Math.floor(delta / 1000)}s`;
  if (delta < 3_600_000) return `${Math.floor(delta / 60_000)}m`;
  return `${Math.floor(delta / 3_600_000)}h`;
}

export function CoordinationPanel({ bridge, bridgeReady }: Props) {
  const [tab, setTab] = useState<TabKey>('overview');
  const [snapshot, setSnapshot] = useState<CoordinationSnapshot>(emptySnapshot);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [taskTitle, setTaskTitle] = useState('Review coordination kernel UI');
  const [lockName, setLockName] = useState('README.md');
  const [contextPack, setContextPack] = useState<ContextPack | null>(null);
  const [actionBusy, setActionBusy] = useState(false);
  const [selectedTaskId, setSelectedTaskId] = useState<string | null>(null);
  const [selectedLockId, setSelectedLockId] = useState<string | null>(null);

  const selectedTask = useMemo(
    () => snapshot.tasks.find((task) => task.id === selectedTaskId) ?? snapshot.tasks[0],
    [selectedTaskId, snapshot.tasks],
  );
  const selectedLock = useMemo(
    () => snapshot.locks.find((lock) => lock.resource_id === selectedLockId) ?? snapshot.locks[0],
    [selectedLockId, snapshot.locks],
  );

  const refresh = useCallback(async () => {
    if (!bridge || !bridgeReady) {
      setSnapshot(emptySnapshot);
      setContextPack(null);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const [workspace, tasks, locks, audit, agents] = await Promise.all([
        bridge.getWorkspaceState(),
        bridge.listTasks(),
        bridge.listLocks(),
        bridge.getAudit(),
        bridge.listAgentContexts(),
      ]);
      setSnapshot({ workspace, tasks, locks, audit, agents });
      setSelectedTaskId((current) => current && tasks.some((task) => task.id === current) ? current : tasks[0]?.id ?? null);
      setSelectedLockId((current) => current && locks.some((lock) => lock.resource_id === current) ? current : locks[0]?.resource_id ?? null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, [bridge, bridgeReady]);

  useEffect(() => {
    void refresh();
    if (!bridgeReady) return undefined;
    const id = window.setInterval(() => void refresh(), 5000);
    return () => window.clearInterval(id);
  }, [bridgeReady, refresh]);

  const createTask = useCallback(async () => {
    if (!bridge || !taskTitle.trim()) return;
    setActionBusy(true);
    try {
      await bridge.createTask({ title: taskTitle.trim(), exclusive: true });
      await refresh();
      setTab('tasks');
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setActionBusy(false);
    }
  }, [bridge, refresh, taskTitle]);

  const claimFirstTask = useCallback(async () => {
    if (!bridge || !selectedTask) return;
    setActionBusy(true);
    try {
      await bridge.claimTask(selectedTask.id, { agent_id: 'ui-operator', lease_ms: 300_000 });
      await refresh();
      setTab('tasks');
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setActionBusy(false);
    }
  }, [bridge, refresh, selectedTask]);

  const completeSelectedTask = useCallback(async () => {
    if (!bridge || !selectedTask) return;
    setActionBusy(true);
    try {
      await bridge.completeTask(selectedTask.id, {
        agent_id: selectedTask.claimed_by ?? 'ui-operator',
        evidence: 'Completed from Coordination panel.',
      });
      await refresh();
      setTab('tasks');
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setActionBusy(false);
    }
  }, [bridge, refresh, selectedTask]);

  const blockSelectedTask = useCallback(async () => {
    if (!bridge || !selectedTask) return;
    setActionBusy(true);
    try {
      await bridge.blockTask(selectedTask.id, {
        agent_id: selectedTask.claimed_by ?? 'ui-operator',
        reason: 'Blocked from Coordination panel.',
      });
      await refresh();
      setTab('tasks');
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setActionBusy(false);
    }
  }, [bridge, refresh, selectedTask]);

  const acquireLock = useCallback(async () => {
    if (!bridge || !lockName.trim()) return;
    setActionBusy(true);
    try {
      await bridge.acquireResourceLock({
        resource_type: 'file',
        name: lockName.trim(),
        owner_id: 'ui-operator',
        lease_ms: 300_000,
      });
      await refresh();
      setTab('locks');
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setActionBusy(false);
    }
  }, [bridge, lockName, refresh]);

  const releaseFirstLock = useCallback(async () => {
    if (!bridge || !selectedLock) return;
    const lock = selectedLock;
    const [, name = lock.resource_id] = lock.resource_id.split(/:(.*)/);
    setActionBusy(true);
    try {
      await bridge.releaseResourceLock({
        resource_type: lock.resource_type,
        name,
        owner_id: lock.owner,
      });
      await refresh();
      setTab('locks');
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setActionBusy(false);
    }
  }, [bridge, refresh, selectedLock]);

  const buildContextPack = useCallback(async () => {
    if (!bridge) return;
    setActionBusy(true);
    try {
      const pack = await bridge.buildContextPack({
        task_id: selectedTask?.id,
        agent_id: selectedTask?.claimed_by ?? 'ui-operator',
        manager_instructions: 'Summarize the task, owned resources, and expected evidence.',
        raw_scrollback: snapshot.audit.map((entry) => entry.event_type).join('\n'),
      });
      setContextPack(pack);
      setTab('context');
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setActionBusy(false);
    }
  }, [bridge, selectedTask, snapshot.audit]);

  const deleteContextPack = useCallback(() => {
    setContextPack(null);
  }, []);

  const recentAudit = useMemo(() => snapshot.audit.slice(-8).reverse(), [snapshot.audit]);

  return (
    <section className="pm-coordination">
      <div className="pm-coordination-head">
        <div>
          <h2>Coordination</h2>
          <p>{bridgeReady ? 'Rust kernel projections' : 'Bridge unavailable'}</p>
        </div>
        <button type="button" onClick={() => void refresh()} disabled={!bridgeReady || loading}>
          {loading ? 'Syncing' : 'Refresh'}
        </button>
      </div>

      {error && <div className="pm-coordination-error">{error}</div>}

      <div className="pm-coordination-tabs" role="tablist" aria-label="Coordination views">
        {(['overview', 'tasks', 'locks', 'audit', 'context', 'agents'] as TabKey[]).map((key) => (
          <button
            key={key}
            type="button"
            className={tab === key ? 'active' : ''}
            onClick={() => setTab(key)}
          >
            {key}
          </button>
        ))}
      </div>

      <div className="pm-coordination-body">
        {tab === 'overview' && (
          <div className="pm-coordination-overview">
            <Metric label="Panes" value={String(snapshot.workspace?.panes.length ?? 0)} />
            <Metric label="Tasks" value={String(snapshot.tasks.length)} />
            <Metric label="Locks" value={String(snapshot.locks.length)} />
            <Metric label="Events" value={String(snapshot.audit.length)} />
          </div>
        )}

        {tab === 'tasks' && (
          <div className="pm-coordination-stack">
            <div className="pm-coordination-inline">
              <input
                value={taskTitle}
                onChange={(event) => setTaskTitle(event.target.value)}
                placeholder="Task title"
              />
              <button type="button" disabled={!bridgeReady || actionBusy} onClick={() => void createTask()}>
                Create
              </button>
              <button type="button" disabled={!selectedTask || actionBusy} onClick={() => void claimFirstTask()}>
                Claim
              </button>
              <button type="button" disabled={!selectedTask || actionBusy} onClick={() => void completeSelectedTask()}>
                Done
              </button>
              <button type="button" disabled={!selectedTask || actionBusy} onClick={() => void blockSelectedTask()}>
                Block
              </button>
            </div>
            {selectedTask && (
              <div className="pm-coordination-selection">
                Selected task: <code>{shortId(selectedTask.id)}</code> · {selectedTask.status}
              </div>
            )}
            <DataList
              empty="No task events yet."
              selectedId={selectedTask?.id}
              onSelect={setSelectedTaskId}
              items={snapshot.tasks.map((task) => ({
                id: task.id,
                title: task.title || shortId(task.id),
                meta: `${task.status}${task.claimed_by ? ` · ${task.claimed_by}` : ''}`,
              }))}
            />
          </div>
        )}

        {tab === 'locks' && (
          <div className="pm-coordination-stack">
            <div className="pm-coordination-inline">
              <input
                value={lockName}
                onChange={(event) => setLockName(event.target.value)}
                placeholder="file path"
              />
              <button type="button" disabled={!bridgeReady || actionBusy} onClick={() => void acquireLock()}>
                Lock
              </button>
              <button type="button" disabled={snapshot.locks.length === 0 || actionBusy} onClick={() => void releaseFirstLock()}>
                Release
              </button>
            </div>
            {selectedLock && (
              <div className="pm-coordination-selection">
                Selected lock: <code>{shortId(selectedLock.resource_id)}</code> · {selectedLock.owner}
              </div>
            )}
            <DataList
              empty="No resource locks."
              selectedId={selectedLock?.resource_id}
              onSelect={setSelectedLockId}
              items={snapshot.locks.map((lock) => ({
                id: lock.resource_id,
                title: lock.resource_id,
                meta: `${lock.resource_type} · ${lock.owner}`,
              }))}
            />
          </div>
        )}

        {tab === 'audit' && (
          <DataList
            empty="No audit events yet."
            items={recentAudit.map((entry) => ({
              id: entry.event_id,
              title: entry.event_type,
              meta: `${entry.actor} · ${relativeTime(entry.timestamp_ms)} ago`,
            }))}
          />
        )}

        {tab === 'context' && (
          <div className="pm-coordination-stack">
            <div className="pm-coordination-inline">
              <button type="button" disabled={!bridgeReady || actionBusy} onClick={() => void buildContextPack()}>
                Build context pack
              </button>
              <button type="button" disabled={!contextPack || actionBusy} onClick={deleteContextPack}>
                Delete
              </button>
              {contextPack && (
                <span className="pm-coordination-byte">
                  {contextPack.context_pack_bytes}/{contextPack.estimated_raw_scrollback_bytes || contextPack.context_pack_bytes} bytes
                </span>
              )}
            </div>
            {contextPack ? (
              <pre className="pm-coordination-pre">{contextPack.prompt}</pre>
            ) : (
              <div className="pm-coordination-empty">No context pack built yet.</div>
            )}
          </div>
        )}

        {tab === 'agents' && (
          <DataList
            empty="No agent contexts loaded."
            items={snapshot.agents.map((agent) => ({
              id: agent.agent_type,
              title: `${agent.label} · ${agent.smartness}`,
              meta: agent.best_for.slice(0, 2).join(', '),
            }))}
          />
        )}
      </div>
    </section>
  );
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className="pm-coordination-metric">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function DataList({
  empty,
  items,
  selectedId,
  onSelect,
}: {
  empty: string;
  items: Array<{ id: string; title: string; meta: string }>;
  selectedId?: string;
  onSelect?: (id: string) => void;
}) {
  if (items.length === 0) {
    return <div className="pm-coordination-empty">{empty}</div>;
  }
  return (
    <ul className="pm-coordination-list">
      {items.map((item) => (
        <li key={item.id} className={selectedId === item.id ? 'selected' : undefined}>
          {onSelect ? (
            <button
              type="button"
              className="pm-coordination-row-button"
              onClick={() => onSelect(item.id)}
            >
              <RowContent item={item} />
            </button>
          ) : (
            <RowContent item={item} />
          )}
        </li>
      ))}
    </ul>
  );
}

function RowContent({ item }: { item: { id: string; title: string; meta: string } }) {
  return (
    <>
          <span className="pm-coordination-list-main">
            <strong>{item.title}</strong>
            <small>{item.meta}</small>
          </span>
          <code>{shortId(item.id)}</code>
    </>
  );
}
