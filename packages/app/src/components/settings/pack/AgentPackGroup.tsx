// Settings > MCP & bridge > "Add Puppet Master to your agents": install the skills and the connection into
// each agent that is on this computer. Review first, then install; Undo puts every file back.
import { useCallback, useEffect, useState } from 'react';
import {
  agentPackApi,
  groupAgents,
  pendingCount,
  visibleRows,
  type AgentPackApi,
  type FileKind,
  type PackReport,
  type PackRow,
  type UndoReport,
} from '../../../lib/agent-pack';
import { errText } from '../../../lib/toolbelt';
import { btn, btnPrimary, overlay, sheet } from '../toolbelt/styles';

const FILE_TEXT: Record<FileKind, string> = { add: 'new', update: 'newer version', replace: 'replaces your file (kept for Undo)', unchanged: 'up to date' };

function names(row: PackRow): string {
  return row.labels.join(', ');
}

function skillsLine(row: PackRow): string {
  const s = row.skills;
  if (!s) return '';
  if (s.status === 'skipped') return s.reason ?? 'Skipped';
  if (s.status === 'error') return s.reason ?? 'Something went wrong';
  if (s.status === 'unchanged') return 'Skills are already installed';
  const n = s.files.filter((f) => f.kind !== 'unchanged').length;
  const replaced = s.files.some((f) => f.kind === 'replace');
  return `${n} ${n === 1 ? 'skill' : 'skills'} to add${replaced ? ', one replaces a file of yours' : ''}`;
}

function mcpLine(row: PackRow): string {
  const m = row.mcp;
  if (!m) return '';
  if (m.status === 'skipped') return m.reason ?? 'Skipped';
  if (m.status === 'error') return m.reason ?? 'Something went wrong';
  if (m.status === 'unchanged') return 'Already connected to Puppet Master';
  return 'Connection to Puppet Master will be added';
}

function PackSheet({ api, onClose, onDone }: { api: AgentPackApi; onClose: () => void; onDone: (text: string, report: PackReport) => void }) {
  const [report, setReport] = useState<PackReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [open, setOpen] = useState<string | null>(null);

  useEffect(() => {
    api.preview().then(setReport, (e) => setError(errText(e)));
  }, [api]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  const rows = report ? visibleRows(groupAgents(report.agents)) : [];
  const absent = report ? groupAgents(report.agents).filter((r) => !rows.includes(r)) : [];
  const pending = pendingCount(rows);

  const install = async () => {
    setBusy(true);
    try {
      const done = await api.apply();
      const written = visibleRows(groupAgents(done.agents)).filter((r) => r.skills?.written || r.mcp?.written);
      const failed = done.agents.filter((a) => a.skills?.status === 'error' || a.mcp?.status === 'error');
      const where = written.map(names).join(', ');
      const text =
        written.length === 0
          ? 'Every agent already has Puppet Master.'
          : `Added to ${where}.${failed.length > 0 ? ` ${failed.length} could not be written.` : ''} Restart open agents to pick it up.`;
      onDone(text, done);
    } catch (e) {
      setError(errText(e));
      setBusy(false);
    }
  };

  return (
    <div className={overlay} onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div role="dialog" aria-modal="true" aria-label="Add Puppet Master to your agents" className={sheet}>
        <div className="border-b border-pm-border px-4 py-3">
          <h4 className="text-sm font-semibold">Add Puppet Master to your agents</h4>
          <p className="mt-1 text-xs leading-5 text-pm-muted">
            Skills are short guides that teach an agent when and how to hand work to other agents. Nothing runs at install time. An
            existing connection is never changed, and Undo puts every file back.
          </p>
        </div>

        <div className="min-h-0 flex-1 space-y-3 overflow-y-auto px-4 py-3">
          {error && (
            <p role="alert" className="rounded-lg border border-pm-border bg-pm-bg px-3 py-2 text-xs text-pm-text">
              {error}
            </p>
          )}
          {!report && !error && <p className="text-xs text-pm-muted">Looking for your agents…</p>}
          {rows.map((row) => (
            <div key={row.key} data-testid="pack-agent" className="rounded-lg border border-pm-border bg-pm-bg">
              <div className="px-3 pt-2.5 text-sm font-medium">{names(row)}</div>
              <dl className="space-y-1 px-3 py-2 text-xs">
                {row.skills && (
                  <div className="flex gap-2">
                    <dt className="w-20 shrink-0 text-pm-muted">Skills</dt>
                    <dd className="min-w-0">
                      <div>{skillsLine(row)}</div>
                      <div className="truncate font-mono text-[11px] text-pm-muted" title={row.skills.path}>
                        {row.skills.path}
                      </div>
                      {open === `${row.key}|skills` &&
                        row.skills.files.map((f) => (
                          <div key={f.name} className="font-mono text-[11px] text-pm-muted">
                            {f.name} · {FILE_TEXT[f.kind]}
                          </div>
                        ))}
                    </dd>
                  </div>
                )}
                {row.mcp && (
                  <div className="flex gap-2">
                    <dt className="w-20 shrink-0 text-pm-muted">Connection</dt>
                    <dd className="min-w-0">
                      <div>{mcpLine(row)}</div>
                      <div className="truncate font-mono text-[11px] text-pm-muted" title={row.mcp.path}>
                        {row.mcp.path}
                      </div>
                      {open === `${row.key}|mcp` && row.mcp.diff && (
                        <pre className="mt-1 max-h-40 overflow-auto whitespace-pre-wrap break-all rounded-md bg-pm-panel p-2 font-mono text-[11px] leading-5">
                          {row.mcp.diff}
                        </pre>
                      )}
                    </dd>
                  </div>
                )}
              </dl>
              <div className="border-t border-pm-border px-3 py-2">
                <button
                  type="button"
                  className="text-xs text-pm-muted underline"
                  aria-expanded={open?.startsWith(row.key) ?? false}
                  onClick={() => setOpen(open?.startsWith(row.key) ? null : `${row.key}|${row.mcp?.diff ? 'mcp' : 'skills'}`)}
                >
                  {open?.startsWith(row.key) ? 'Hide details' : 'Show details'}
                </button>
              </div>
            </div>
          ))}
          {report && rows.length === 0 && <p className="text-xs text-pm-muted">No supported agents found on this computer.</p>}
          {absent.length > 0 && (
            <p className="text-xs leading-5 text-pm-muted" data-testid="pack-absent">
              Not on this computer: {absent.map(names).join(', ')}.
            </p>
          )}
        </div>

        <div className="flex items-center justify-between gap-2 border-t border-pm-border px-4 py-3">
          <span className="text-xs text-pm-muted">
            {report ? (pending === 0 ? 'Nothing to add.' : `${pending} ${pending === 1 ? 'agent' : 'agents'} will change.`) : ''}
          </span>
          <div className="flex gap-2">
            <button type="button" className={btn} onClick={onClose}>
              Cancel
            </button>
            <button type="button" className={btnPrimary} onClick={() => void install()} disabled={busy || pending === 0}>
              {busy ? 'Installing…' : 'Install'}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

export function AgentPackGroup({ api = agentPackApi }: { api?: AgentPackApi }) {
  const [reviewing, setReviewing] = useState(false);
  const [recorded, setRecorded] = useState(0);
  const [found, setFound] = useState<string[] | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [undoing, setUndoing] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const r = await api.preview();
      setRecorded(r.recorded);
      setFound(visibleRows(groupAgents(r.agents)).filter((x) => x.installed).map(names));
      setError(null);
    } catch (e) {
      setError(errText(e));
    }
  }, [api]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const undo = async () => {
    setUndoing(true);
    try {
      const u: UndoReport = await api.undo();
      const n = u.removed.length + u.restored.length;
      setNotice(
        u.skipped.length > 0
          ? `Undone for ${n} ${n === 1 ? 'item' : 'items'}. ${u.skipped.length} you changed since were left as they are.`
          : `Undone. ${n} ${n === 1 ? 'item was' : 'items were'} put back as before.`,
      );
      await refresh();
    } catch (e) {
      setError(errText(e));
    } finally {
      setUndoing(false);
    }
  };

  return (
    <section aria-label="Add Puppet Master to your agents" className="rounded-xl border border-pm-border bg-pm-panel p-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <h3 className="text-sm font-semibold">Add Puppet Master to your agents</h3>
          <p className="mt-1 max-w-xl text-sm leading-6 text-pm-muted">
            Teach each agent you use when to ask another agent for help, and connect it to Puppet Master. You review every file first.
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          {recorded > 0 && (
            <button type="button" className={btn} onClick={() => void undo()} disabled={undoing}>
              {undoing ? 'Undoing…' : 'Undo'}
            </button>
          )}
          <button type="button" className={btnPrimary} onClick={() => setReviewing(true)}>
            Review and install
          </button>
        </div>
      </div>
      {found && <p className="mt-3 text-xs leading-5 text-pm-muted">{found.length > 0 ? `Found: ${found.join(', ')}.` : 'No supported agents found yet.'}</p>}
      {error && (
        <p role="alert" className="mt-3 rounded-lg border border-pm-border bg-pm-bg px-3 py-2 text-xs leading-5 text-pm-text">
          {error}
        </p>
      )}
      {notice && <p className="mt-3 text-xs leading-5 text-pm-muted">{notice}</p>}
      {reviewing && (
        <PackSheet
          api={api}
          onClose={() => setReviewing(false)}
          onDone={(text, done) => {
            setReviewing(false);
            setNotice(text);
            setRecorded(done.recorded);
            void refresh();
          }}
        />
      )}
    </section>
  );
}
