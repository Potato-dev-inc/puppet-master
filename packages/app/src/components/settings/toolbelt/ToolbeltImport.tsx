// "Import from agents": offer the servers an agent already has, so they can join the toolbelt.
import { useEffect, useState } from 'react';
import { errText, type ImportCandidate, type ToolbeltApi } from '../../../lib/toolbelt';
import { btn, btnPrimary, overlay, sheet } from './styles';

export function ToolbeltImport({
  api,
  projectPath,
  onClose,
  onImported,
}: {
  api: ToolbeltApi;
  projectPath: string | null;
  onClose: () => void;
  onImported: (count: number) => void;
}) {
  const [found, setFound] = useState<ImportCandidate[] | null>(null);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api
      .importPreview(projectPath)
      .then((list) => {
        setFound(list);
        setPicked(new Set(list.filter((c) => !c.already).map((c) => c.reference)));
      })
      .catch((e) => setError(errText(e)));
  }, [api, projectPath]);

  const run = async () => {
    setBusy(true);
    try {
      const added = await api.importApply([...picked], projectPath);
      onImported(added.length);
    } catch (e) {
      setError(errText(e));
      setBusy(false);
    }
  };

  const toggle = (ref: string) =>
    setPicked((prev) => {
      const next = new Set(prev);
      if (next.has(ref)) next.delete(ref);
      else next.add(ref);
      return next;
    });

  return (
    <div className={overlay} onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div role="dialog" aria-modal="true" aria-label="Import from agents" className={sheet}>
        <div className="border-b border-pm-border px-4 py-3">
          <h4 className="text-sm font-semibold">Import from agents</h4>
          <p className="mt-1 text-xs leading-5 text-pm-muted">
            These servers are already set up in your agents. Pick the ones to add to your toolbelt. Nothing is changed in the agents.
          </p>
        </div>
        <div className="min-h-0 flex-1 space-y-2 overflow-y-auto px-4 py-3">
          {error && (
            <p role="alert" className="rounded-lg border border-pm-border bg-pm-bg px-3 py-2 text-xs">
              {error}
            </p>
          )}
          {!found && !error && <p className="text-xs text-pm-muted">Reading agent configs…</p>}
          {found?.length === 0 && <p className="text-xs text-pm-muted">No servers found in your agents&apos; configs.</p>}
          {found?.map((c) => (
            <label key={c.reference} className="flex items-start gap-3 rounded-lg border border-pm-border bg-pm-bg px-3 py-2.5">
              <input type="checkbox" className="mt-1" disabled={c.already} checked={picked.has(c.reference)} onChange={() => toggle(c.reference)} />
              <span className="min-w-0 flex-1">
                <span className="block text-sm font-medium">{c.server.name}</span>
                <span className="block truncate font-mono text-[11px] text-pm-muted">
                  {c.server.transport === 'http' ? c.server.url : [c.server.command, ...(c.server.args ?? [])].join(' ')}
                </span>
                <span className="block text-[11px] text-pm-muted">
                  {c.already ? 'Already in your toolbelt · ' : ''}Found in {c.agents.map((a) => a.label).join(', ')}
                </span>
              </span>
            </label>
          ))}
        </div>
        <div className="flex justify-end gap-2 border-t border-pm-border px-4 py-3">
          <button type="button" className={btn} onClick={onClose}>
            Cancel
          </button>
          <button type="button" className={btnPrimary} disabled={busy || picked.size === 0} onClick={() => void run()}>
            {busy ? 'Adding…' : 'Add to toolbelt'}
          </button>
        </div>
      </div>
    </div>
  );
}
