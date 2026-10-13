// The "Apply to all agents" sheet: what will change in each agent's config, then Apply.
import { useCallback, useEffect, useState } from 'react';
import { changeCounts, errText, type ChangeKind, type SyncHost, type SyncReport, type ToolbeltApi } from '../../../lib/toolbelt';
import { btn, btnPrimary, overlay, sheet } from './styles';

const KIND_TEXT: Record<ChangeKind, string> = { add: 'Add', update: 'Update', remove: 'Remove', unchanged: 'No change' };

function hostTitle(h: SyncHost): string {
  return h.agents.map((a) => a.label).join(', ');
}

function hostLine(h: SyncHost): string {
  if (h.status === 'skipped') return h.reason ?? 'Skipped';
  if (h.status === 'error') return h.reason ?? 'Something went wrong';
  if (h.status === 'unchanged') return 'Already up to date';
  const n = changeCounts(h);
  return [n.add && `${n.add} to add`, n.update && `${n.update} to update`, n.remove && `${n.remove} to remove`].filter(Boolean).join(', ');
}

export function ToolbeltPreview({
  api,
  projectPath,
  onClose,
  onApplied,
}: {
  api: ToolbeltApi;
  projectPath: string | null;
  onClose: () => void;
  onApplied: (summary: string) => void;
}) {
  const [report, setReport] = useState<SyncReport | null>(null);
  const [includePm, setIncludePm] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [open, setOpen] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setError(null);
      setReport(await api.syncPreview({ project: projectPath, includePm }));
    } catch (e) {
      setError(errText(e));
    }
  }, [api, projectPath, includePm]);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  const pending = report?.hosts.filter((h) => h.status === 'ready') ?? [];

  const apply = async () => {
    setBusy(true);
    try {
      const done = await api.syncApply({ project: projectPath, includePm });
      const written = done.hosts.filter((h) => h.written);
      const failed = done.hosts.filter((h) => h.status === 'error');
      const files = `${written.length} config ${written.length === 1 ? 'file' : 'files'}`;
      const text =
        failed.length > 0
          ? `Updated ${files}; ${failed.length} could not be written. Restart open agents to pick up changes.`
          : written.length === 0
            ? 'Every agent is already up to date.'
            : `Updated ${files}. Restart open agents to pick up changes.`;
      onApplied(text);
    } catch (e) {
      setError(errText(e));
      setBusy(false);
    }
  };

  return (
    <div className={overlay} onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div role="dialog" aria-modal="true" aria-label="Apply to all agents" className={sheet}>
        <div className="border-b border-pm-border px-4 py-3">
          <h4 className="text-sm font-semibold">Apply to all agents</h4>
          <p className="mt-1 text-xs leading-5 text-pm-muted">
            This only adds, updates or removes entries named <span className="font-mono">pm-toolbelt-…</span>. Your own entries are not
            touched. The first time a file changes, a <span className="font-mono">.bak</span> copy is saved next to it.
          </p>
        </div>

        <div className="min-h-0 flex-1 space-y-3 overflow-y-auto px-4 py-3">
          {error && (
            <p role="alert" className="rounded-lg border border-pm-border bg-pm-bg px-3 py-2 text-xs text-pm-text">
              {error}
            </p>
          )}
          {!report && !error && <p className="text-xs text-pm-muted">Checking each agent…</p>}
          {report?.hosts.map((h) => {
            const id = `${h.path}|${h.scope}`;
            const changed = h.changes.filter((c) => c.kind !== 'unchanged');
            return (
              <div key={id} data-testid="toolbelt-host" className="rounded-lg border border-pm-border bg-pm-bg">
                <div className="flex flex-wrap items-baseline justify-between gap-2 px-3 pt-2.5">
                  <span className="text-sm font-medium">{hostTitle(h)}</span>
                  <span className="text-xs text-pm-muted">{hostLine(h)}</span>
                </div>
                <div className="truncate px-3 pb-2 font-mono text-[11px] text-pm-muted">
                  {h.path}
                  {h.scope === 'project' ? ' (this project)' : ''}
                </div>
                {changed.length > 0 && (
                  <ul className="border-t border-pm-border px-3 py-2 text-xs">
                    {changed.map((c) => (
                      <li key={c.key} className="flex gap-2 py-0.5">
                        <span className="w-14 shrink-0 text-pm-muted">{KIND_TEXT[c.kind]}</span>
                        <span className="truncate">{c.name}</span>
                      </li>
                    ))}
                  </ul>
                )}
                {h.notes.map((n) => (
                  <p key={n} className="border-t border-pm-border px-3 py-2 text-xs text-pm-muted">
                    {n}
                  </p>
                ))}
                {h.diff && (
                  <div className="border-t border-pm-border px-3 py-2">
                    <button
                      type="button"
                      className="text-xs text-pm-muted underline"
                      aria-expanded={open === id}
                      onClick={() => setOpen(open === id ? null : id)}
                    >
                      {open === id ? 'Hide file changes' : 'Show file changes'}
                    </button>
                    {open === id && (
                      <pre className="mt-2 max-h-48 overflow-auto whitespace-pre-wrap break-all rounded-md bg-pm-panel p-2 font-mono text-[11px] leading-5">
                        {h.diff}
                      </pre>
                    )}
                  </div>
                )}
              </div>
            );
          })}
          {report && report.unsupported.length > 0 && (
            <div className="rounded-lg border border-pm-border bg-pm-bg px-3 py-2.5" data-testid="toolbelt-unsupported">
              <div className="text-sm font-medium">Not supported</div>
              <ul className="mt-1 space-y-1 text-xs text-pm-muted">
                {report.unsupported.map((u) => (
                  <li key={u.id}>
                    <span className="text-pm-text">{u.label}</span>: {u.reason}
                  </li>
                ))}
              </ul>
            </div>
          )}
          <label className="flex items-center gap-2 text-xs text-pm-muted">
            <input type="checkbox" checked={includePm} onChange={(e) => setIncludePm(e.target.checked)} />
            Also register Puppet Master itself in these agents
          </label>
        </div>

        <div className="flex items-center justify-between gap-2 border-t border-pm-border px-4 py-3">
          <span className="text-xs text-pm-muted">
            {report ? (pending.length === 0 ? 'Nothing to change.' : `${pending.length} ${pending.length === 1 ? 'file' : 'files'} will change.`) : ''}
          </span>
          <div className="flex gap-2">
            <button type="button" className={btn} onClick={onClose}>
              Cancel
            </button>
            <button type="button" className={btnPrimary} onClick={() => void apply()} disabled={busy || pending.length === 0}>
              {busy ? 'Applying…' : 'Apply'}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
