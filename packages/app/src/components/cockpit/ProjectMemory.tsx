// "Project memory": the one memory every agent in a project reads. The text is saved to
// `<project>/.puppet-master/memory.md` and rendered into each agent's own instruction file.
import { useCallback, useEffect, useState } from 'react';
import { memoryApi, type MemoryApi, type MemoryTargetStatus, type MemoryView } from '../../lib/memory-api';
import { projectName } from '../../lib/mobile-agent-tabs';
import { agentLabel, useAgentRegistry } from '../../lib/agentRegistry';
import { ProjectCopies } from './ProjectCopies';
import { SegTabs } from './SegTabs';
import { projectOfCwd } from '../../lib/ship-model';

const STATUS_TEXT: Record<MemoryTargetStatus, string> = {
  in_sync: 'In sync',
  will_create: 'Will create',
  will_add: 'Will add a block',
  will_update: 'Will update',
  will_remove: 'Will remove',
  nothing: 'Nothing to write',
  blocked: 'Markers damaged',
};

function errText(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

export function ProjectMemory({
  project,
  api = memoryApi,
  onClose,
}: {
  /** Folder of the selected pane; the backend falls back to the open project. */
  project?: string;
  api?: MemoryApi;
  onClose: () => void;
}) {
  const listing = useAgentRegistry();
  const [view, setView] = useState<MemoryView | null>(null);
  const [draft, setDraft] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [tab, setTab] = useState<'memory' | 'copies'>('memory');

  const apply = useCallback((next: MemoryView, keepDraft = false) => {
    setView(next);
    if (!keepDraft) setDraft(next.memory);
    setError(null);
  }, []);

  useEffect(() => {
    let live = true;
    api
      .get(project)
      .then((v) => live && apply(v))
      .catch((e) => live && setError(errText(e)));
    return () => {
      live = false;
    };
  }, [api, project, apply]);

  const dirty = view !== null && draft !== view.memory;

  const run = async (job: () => Promise<MemoryView>, done: string, keepDraft = false) => {
    setBusy(true);
    setNote(null);
    try {
      const next = await job();
      apply(next, keepDraft);
      const problems = next.report?.errors ?? [];
      if (problems.length > 0) setError(problems.join(' '));
      else setNote(done);
    } catch (e) {
      setError(errText(e));
    } finally {
      setBusy(false);
    }
  };

  const save = () => run(() => api.save(draft, project), 'Saved. Agents in this project will read it.');
  const sync = () => run(() => api.sync(project), 'Synced.', true);

  const onKeyDown = (e: React.KeyboardEvent) => {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
      e.preventDefault();
      if (dirty && !busy) void save();
    }
  };

  const off = view !== null && (!view.global_enabled || !view.project_enabled);

  return (
    <div className="ck-memory" onKeyDown={onKeyDown}>
      <header className="ck-memory-head">
        <div>
          <h1 className="ck-title">Project memory</h1>
          <p className="ck-sub">Every agent in this project reads this.</p>
        </div>
        <button type="button" className="ck-btn ck-btn--plain" onClick={onClose}>
          Close
        </button>
      </header>

      <div className="ck-memory-tabs">
        <SegTabs
          label="Project"
          value={tab}
          onChange={setTab}
          options={[
            { id: 'memory', label: 'Memory' },
            { id: 'copies', label: 'Copies' },
          ]}
          small
        />
      </div>

      {view && <p className="ck-memory-project">{projectName(view.project)}</p>}

      {tab === 'copies' && <ProjectCopies project={view?.project ?? projectOfCwd(project)?.path} />}

      {tab === 'memory' && (
        <>
      <textarea
        className="ck-memory-editor"
        aria-label="Project memory"
        value={draft}
        placeholder={'What every agent should know about this project.\nStack, commands, conventions, decisions.'}
        spellCheck={false}
        onChange={(e) => setDraft(e.target.value)}
        disabled={view === null}
      />

      <div className="ck-memory-actions">
        <button type="button" className="ck-btn ck-btn--primary" disabled={!dirty || busy} onClick={() => void save()}>
          Save
        </button>
        <button type="button" className="ck-btn ck-btn--secondary" disabled={view === null || busy} onClick={() => void sync()}>
          Sync now
        </button>
        <span className="ck-memory-msg" role="status" aria-live="polite">
          {error ? <span className="ck-memory-err">{error}</span> : (note ?? (dirty ? 'Unsaved changes' : ''))}
        </span>
      </div>

      <section aria-label="Agent files">
        <h2 className="ck-section">Files agents read</h2>
        {off && (
          <p className="ck-sub ck-memory-off">
            {view && !view.global_enabled
              ? 'Shared project memory is off in Settings.'
              : 'Shared project memory is off for this project.'}{' '}
            Agent files carry no memory block.
          </p>
        )}
        <ul className="ck-memory-files">
          {(view?.targets ?? []).map((t) => (
            <li key={t.path} className="ck-memory-file">
              <span className="ck-memory-path">{t.path}</span>
              <span className="ck-memory-agents">{t.agents.map((a) => agentLabel(listing, a)).join(', ')}</span>
              {t.user_content && <span className="ck-memory-tag">Has your own text</span>}
              <span className={`ck-memory-status is-${t.status}`}>{STATUS_TEXT[t.status]}</span>
            </li>
          ))}
        </ul>
        <p className="ck-sub ck-memory-fine">Only the block between the Puppet Master markers is ever changed.</p>
      </section>

      <label className="ck-memory-switch">
        <input
          type="checkbox"
          checked={view?.project_enabled ?? true}
          disabled={view === null || busy}
          onChange={(e) =>
            void run(
              () => api.setProjectEnabled(e.target.checked, project),
              e.target.checked ? 'Shared memory is on for this project.' : 'Removed from agent files.',
              true,
            )
          }
        />
        <span>Share memory in this project</span>
      </label>
        </>
      )}
    </div>
  );
}
