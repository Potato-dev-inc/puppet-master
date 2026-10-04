// Settings > Coordination storage: project-scoped task/lock/event-log storage info.
import { useEffect, useState } from 'react';
import { tauri, type CoordinationStorageInfo } from '../../../lib/tauri';
import { SettingBlock, SettingToggle, SettingsSection } from '../settings-ui';
import type { SettingsTabContext } from './tab-context';

export function StorageTab({ ctx }: { ctx: SettingsTabContext }) {
  const [storageInfo, setStorageInfo] = useState<CoordinationStorageInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const refresh = async () => {
    try {
      setError(null);
      setStorageInfo(await tauri.getCoordinationStorageInfo());
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };
  useEffect(() => {
    void refresh();
  }, [ctx.projectPath]);
  const scopeLabel = storageInfo?.scope === 'project' ? 'Project-local' : 'Global fallback';
  return (
    <SettingsSection title="Coordination storage" description="Task board, resource locks, audit entries, and pane timeline events are scoped to the selected project folder.">
      <SettingBlock label="Workspace session" implemented description="Coordination events are replayed from this JSONL file to rebuild tasks and locks.">
        <div className="space-y-3">
          <div className="grid gap-3 sm:grid-cols-3">
            <MetricPill label="Scope" value={scopeLabel} />
            <MetricPill label="Tasks" value={String(storageInfo?.task_count ?? 0)} />
            <MetricPill label="Locks" value={String(storageInfo?.lock_count ?? 0)} />
          </div>
          <div className="rounded-lg border border-pm-border bg-pm-bg p-3">
            <div className="text-xs font-medium uppercase tracking-wide text-pm-muted">Project</div>
            <div className="mt-1 break-all font-mono text-xs text-pm-text">
              {storageInfo?.project_path ?? ctx.projectPath ?? 'No project folder selected'}
            </div>
          </div>
          <div className="rounded-lg border border-pm-border bg-pm-bg p-3">
            <div className="text-xs font-medium uppercase tracking-wide text-pm-muted">Event log</div>
            <div className="mt-1 break-all font-mono text-xs text-pm-text">
              {storageInfo?.event_log_path ?? 'Loading...'}
            </div>
            <div className="mt-2 text-xs text-pm-muted">
              {storageInfo?.event_count ?? 0} events · {storageInfo?.exists ? 'file exists' : 'created on first write'}
            </div>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <button type="button" onClick={() => void refresh()} className="rounded-lg border border-pm-border px-3 py-2 text-sm hover:bg-pm-border/40">
              Refresh
            </button>
            <span className="break-all text-xs text-pm-muted">
              Directory: {storageInfo?.storage_dir ?? 'Loading...'}
            </span>
          </div>
          {error && (
            <p className="rounded-lg border border-red-500/30 bg-red-500/10 px-3 py-2 text-xs leading-5 text-red-300">
              {error}
            </p>
          )}
        </div>
      </SettingBlock>
      <SettingToggle label="Project-local tasks and locks" description="Keep coordination state inside each project's .puppet-master folder." checked implemented onChange={() => undefined} />
      <SettingToggle label="Require locks for file edits" description="Prompt orchestrators to claim file or directory ownership before delegating edits." checked implemented={false} onChange={() => undefined} />
      <SettingToggle label="Archive completed sessions" description="Move completed task and lock history into dated archive files." checked={false} implemented={false} onChange={() => undefined} />
    </SettingsSection>
  );
}

function MetricPill({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg border border-pm-border bg-pm-bg px-3 py-2">
      <div className="text-xs font-medium uppercase tracking-wide text-pm-muted">{label}</div>
      <div className="mt-1 truncate text-sm font-semibold text-pm-text">{value}</div>
    </div>
  );
}
