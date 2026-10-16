// Settings > Agents > Agent definitions: declarative agent definitions (list, errors, reload, approve/revoke), drafts,
// sources, consented projects and extra directories. One settings row made of several blocks.
// Every string from a definition file is rendered as React text, never as HTML (UI-4).
import { useCallback, useEffect, useState } from 'react';
import {
  approvalWarnings,
  argvDiff,
  canReview,
  errorMessage,
  fileName,
  formatArgv,
  sourceLabel,
  stateLabel,
  tauriAgentDefinitionsApi,
  type AgentDefinitionsApi,
  type DefinitionPreview,
  type DefinitionRow,
  type DefinitionsListing,
  type DraftsListing,
  type FixtureCheck,
  type ProjectScan,
} from '../../../lib/agentDefinitions';
import {
  agentReadyMessage,
  handleDefinitionsChanged,
  onAgentDefinitionsChanged,
  resetAgentRegistryCache,
  listAgentRegistry,
} from '../../../lib/agentRegistry';
import type { FinishResult, WizardApi } from '../../../lib/agentWizard';
import { SettingBlock } from '../settings-ui';
import { AgentWizard } from './AgentWizard';

const STATE_TONE: Record<string, string> = {
  active: 'border-pm-border bg-pm-border/40 text-pm-text',
  unapproved: 'border-pm-border text-pm-muted',
  needs_approval: 'border-pm-border text-pm-muted',
  approval_stale: 'border-pm-border text-pm-muted',
  invalid: 'border-pm-text text-pm-text',
};

const BADGE = 'inline-flex shrink-0 items-center rounded-full border px-2 py-0.5 text-[10px] font-medium';
const BTN =
  'rounded-lg border border-pm-border px-3 py-1.5 text-xs font-medium hover:bg-pm-border/40 disabled:opacity-50';

interface Reviewing {
  row: DefinitionRow;
  preview: DefinitionPreview | null;
  error: string | null;
}

export function AgentDefinitions({
  api = tauriAgentDefinitionsApi,
  wizardApi,
}: {
  api?: AgentDefinitionsApi;
  /** The wizard's backend; tests pass a mock. */
  wizardApi?: WizardApi;
}) {
  const [listing, setListing] = useState<DefinitionsListing | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [reviewing, setReviewing] = useState<Reviewing | null>(null);
  const [fixturesDir, setFixturesDir] = useState('');
  const [genericCommand, setGenericCommand] = useState('');
  const [checks, setChecks] = useState<{ file: string; check: FixtureCheck } | null>(null);
  const [drafts, setDrafts] = useState<DraftsListing | null>(null);
  const [draftCheck, setDraftCheck] = useState<{ path: string; preview: DefinitionPreview } | null>(null);
  const [projectScan, setProjectScan] = useState<ProjectScan | null>(null);
  const [bannerDismissed, setBannerDismissed] = useState(false);
  const [extraPath, setExtraPath] = useState('');
  const [wizardOpen, setWizardOpen] = useState(false);

  const apply = useCallback((next: DefinitionsListing) => {
    setListing(next);
    resetAgentRegistryCache();
  }, []);

  const load = useCallback(async () => {
    try {
      apply(await api.list());
      setError(null);
    } catch (e) {
      setError(errorMessage(e));
    }
  }, [api, apply]);

  const loadDrafts = useCallback(async () => {
    if (!api.draftsList) return;
    try {
      setDrafts(await api.draftsList());
    } catch (e) {
      setError(errorMessage(e));
    }
  }, [api]);

  // The consent banner (P5-3): the open project's definition files, listed by name only.
  const loadProjectScan = useCallback(async () => {
    try {
      const project = (await api.projectPath())?.trim();
      setProjectScan(project ? await api.projectScan(project) : null);
    } catch {
      setProjectScan(null);
    }
  }, [api]);

  useEffect(() => {
    void load();
    void loadDrafts();
    void loadProjectScan();
    // The backend pushes a change when a definition file is saved: no polling.
    return onAgentDefinitionsChanged(() => void load());
  }, [load, loadDrafts, loadProjectScan]);

  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const reload = () =>
    run(async () => {
      const next = await api.reload();
      apply(next);
      const diff = next.diff;
      const parts = diff
        ? [
            diff.added.length ? `added ${diff.added.join(', ')}` : '',
            diff.removed.length ? `removed ${diff.removed.join(', ')}` : '',
            diff.changed.length ? `changed ${diff.changed.join(', ')}` : '',
          ].filter(Boolean)
        : [];
      setNote(parts.length ? `Reloaded: ${parts.join('; ')}` : 'Reloaded: no changes');
    });

  const addGeneric = () =>
    run(async () => {
      const command = genericCommand.trim();
      if (!command) return;
      apply(await api.addGeneric(command));
      setGenericCommand('');
      setNote(`Added ${command} as an experimental generic agent. Review and approve it below.`);
    });

  const exportRow = (row: DefinitionRow) =>
    run(async () => {
      if (!row.id) return;
      const done = await api.exportDefinition(row.id);
      if (done) setNote(`Exported ${row.id} to ${done.path}${done.fixtures_zip ? ` and ${done.fixtures_zip}` : ''}`);
    });

  const importFile = () =>
    run(async () => {
      const next = await api.importDefinition();
      if (!next) return;
      apply(next);
      await handleDefinitionsChanged();
      setNote('Imported. The definition is unapproved: review it and approve it below before it can run.');
    });

  const check = (row: DefinitionRow) =>
    run(async () => {
      setChecks({ file: row.file, check: await api.checkFile(row.file, fixturesDir.trim() || undefined) });
    });

  const createFixtures = (row: DefinitionRow) =>
    run(async () => {
      await api.createFixturesDir(row.file, fixturesDir.trim() || undefined);
      setChecks({ file: row.file, check: await api.checkFile(row.file, fixturesDir.trim() || undefined) });
    });

  const validateDraft = (path: string) =>
    run(async () => {
      if (!api.draftValidate) return;
      setDraftCheck({ path, preview: await api.draftValidate(path) });
    });

  const openReview = async (row: DefinitionRow) => {
    try {
      setReviewing({ row, preview: await api.preview(row.file), error: null });
    } catch (e) {
      setReviewing({ row, preview: null, error: errorMessage(e) });
    }
  };

  const review = (row: DefinitionRow) => run(() => openReview(row));

  // The wizard wrote an UNAPPROVED definition: show the normal approval dialog for it.
  const wizardDone = (done: FinishResult) =>
    run(async () => {
      setWizardOpen(false);
      const listed = done.listing as DefinitionsListing | undefined;
      const next = listed?.definitions ? listed : await api.list();
      apply(next);
      await handleDefinitionsChanged();
      setNote(`The wizard wrote ${done.id}. Check the command below, then approve it.`);
      const row = next.definitions.find((r) => r.id === done.id && r.source === 'user');
      if (row) await openReview(row);
    });

  // The registry is live (backend watcher + `agent_definitions_changed`): no restart is needed after
  // any change here. Refetch now so the spawn menu / Overview update without waiting for the event.
  const approve = () =>
    run(async () => {
      if (!reviewing?.row.fingerprint) return;
      const id = reviewing.row.id;
      await api.approve(reviewing.row.file, reviewing.row.fingerprint);
      setReviewing(null);
      apply(await api.list());
      await handleDefinitionsChanged();
      if (id) setNote(agentReadyMessage(await listAgentRegistry(), id));
    });

  const setConsent = (kind: string, path: string, enabled: boolean) =>
    run(async () => {
      await api.consent(kind, path, enabled);
      apply(await api.list());
      await loadProjectScan();
    });

  const setSource = (name: string, enabled: boolean) =>
    run(async () => {
      await api.sourceSet(name, enabled);
      apply(await api.list());
    });

  const extraDir = (action: 'add' | 'remove', path: string) =>
    run(async () => {
      await api.extraDir(action, path);
      if (action === 'add') setExtraPath('');
      apply(await api.list());
    });

  const revoke = (row: DefinitionRow) =>
    run(async () => {
      if (!row.id) return;
      await api.revoke(row.id);
      apply(await api.list());
      await handleDefinitionsChanged();
      setNote(`${row.id} was revoked - removed from the New session menu. No restart needed.`);
    });

  return (
    <>
      {projectScan && projectScan.files.length > 0 && !projectScan.consented && !bannerDismissed && (
        <ConsentBanner
          scan={projectScan}
          busy={busy}
          onAllow={() => void setConsent('project', projectScan.project, true)}
          onDismiss={() => setBannerDismissed(true)}
        />
      )}
      <SettingBlock
        label="Definitions"
        description="Add your own agents with a small settings file. A definition runs a program, so a new or changed one stays off until you review and approve it here."
      >
        <div className="mb-3 flex items-center gap-2">
          <button type="button" className={BTN} disabled={busy} onClick={() => void reload()}>
            Reload
          </button>
          <button type="button" className={BTN} disabled={busy} onClick={() => void importFile()}>
            Import
          </button>
          <button type="button" className={BTN} disabled={busy} onClick={() => setWizardOpen(true)}>
            Add with wizard
          </button>
          {note && (
            <span role="status" data-testid="agent-toast" className="text-xs text-pm-muted">
              {note}
            </span>
          )}
        </div>
        {error && (
          <p role="alert" className="mb-3 text-sm text-pm-text">
            {error}
          </p>
        )}
        {listing && listing.definitions.length === 0 && (
          <p className="text-sm text-pm-muted">No definition files found.</p>
        )}
        <div className="mb-3 flex items-center gap-2">
          <input
            type="text"
            aria-label="Command name"
            value={genericCommand}
            onChange={(e) => setGenericCommand(e.target.value)}
            placeholder="command name, e.g. mycoder"
            className="min-w-0 flex-1 rounded-lg border border-pm-border bg-transparent px-2 py-1 text-xs"
          />
          <button type="button" className={BTN} disabled={busy || !genericCommand.trim()} onClick={() => void addGeneric()}>
            Add as generic agent
          </button>
        </div>
        <p className="mb-3 text-xs text-pm-muted">
          Experimental: runs the command in a visible pane and reads its screen with shared heuristics. It stays
          inactive until you approve it.
        </p>
        <label className="mb-3 block text-xs text-pm-muted">
          Fixtures folder for Check with fixtures (empty: a folder named after the file, next to it)
          <input
            type="text"
            value={fixturesDir}
            onChange={(e) => setFixturesDir(e.target.value)}
            placeholder="C:\path\to\captures"
            className="mt-1 block w-full rounded-lg border border-pm-border bg-transparent px-2 py-1 text-xs"
          />
        </label>
        <ul className="space-y-3">
          {listing?.definitions.map((row) => (
            <DefinitionItem
              key={row.file}
              row={row}
              busy={busy}
              onReview={review}
              onRevoke={revoke}
              onCheck={check}
              onExport={exportRow}
              check={checks?.file === row.file ? checks.check : null}
              onCreateFixtures={createFixtures}
            />
          ))}
        </ul>
      </SettingBlock>
      {api.draftsList && (
        <SettingBlock
          label="Drafts"
          description="Definitions drafted from a recorded pane session (the Rec button in a pane header). A draft is never loaded or approved: review it, copy it to the agents folder, then approve it above."
        >
          <div className="mb-3 flex items-center gap-2">
            <button type="button" className={BTN} disabled={busy} onClick={() => void loadDrafts()}>
              Refresh drafts
            </button>
            {drafts && <span className="break-all text-xs text-pm-muted">{drafts.dir}</span>}
          </div>
          {drafts && drafts.drafts.length === 0 && <p className="text-sm text-pm-muted">No drafts yet.</p>}
          <ul className="space-y-2">
            {drafts?.drafts.map((draft) => (
              <li key={draft.path} data-testid="draft-row" className="rounded-lg border border-pm-border p-3">
                <div className="flex items-center gap-2">
                  <span className="min-w-0 flex-1 truncate text-sm font-semibold">{draft.name}</span>
                  <button type="button" className={BTN} disabled={busy} onClick={() => void validateDraft(draft.path)}>
                    Validate draft
                  </button>
                </div>
                <p className="mt-1 break-all text-xs text-pm-muted">{draft.path}</p>
                {draftCheck?.path === draft.path && (
                  <div role="status" className="mt-2 text-xs">
                    <p className={draftCheck.preview.ok ? 'text-pm-muted' : 'font-semibold text-pm-text'}>
                      {draftCheck.preview.ok ? 'Draft is valid.' : 'Draft has errors:'}
                    </p>
                    {draftCheck.preview.errors.map((err, i) => (
                      <p key={i} className="text-pm-text">
                        {err.code} {err.path ?? ''}: {err.message}
                      </p>
                    ))}
                  </div>
                )}
              </li>
            ))}
          </ul>
        </SettingBlock>
      )}
      <SettingBlock
        label="Sources"
        description="Where definitions are read from. Project and extra directories only load after you consent to that location."
      >
        <ul className="space-y-2">
          {listing?.sources.map((source) => (
            <li
              key={source.name}
              data-testid="source-row"
              data-source={source.name}
              className="flex items-center gap-3 rounded-lg border border-pm-border p-3"
            >
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2 text-sm font-semibold">
                  {sourceLabel(source.name)}
                  <span className={`${BADGE} border-pm-border text-pm-muted`}>
                    {source.enabled ? 'Enabled' : 'Disabled'}
                  </span>
                </div>
                {source.path && <p className="mt-1 break-all text-xs text-pm-muted">{source.path}</p>}
              </div>
              {source.name !== 'bundled' && (
                <Switch
                  checked={source.enabled}
                  label={`Enable ${sourceLabel(source.name)} definitions`}
                  disabled={busy}
                  onToggle={() => void setSource(source.name, !source.enabled)}
                />
              )}
            </li>
          ))}
        </ul>
        <h4 className="mb-1 mt-4 text-xs font-semibold">Consented projects</h4>
        {(listing?.projects ?? []).length === 0 && (
          <p className="text-xs text-pm-muted">No project is allowed to load definitions.</p>
        )}
        <ul className="space-y-2">
          {(listing?.projects ?? []).map((project) => (
            <li key={project.path} data-testid="project-consent-row" className="flex items-center gap-3 rounded-lg border border-pm-border p-3">
              <div className="min-w-0 flex-1">
                <p className="break-all text-xs">{project.path}</p>
                <p className="text-xs text-pm-muted">{project.files.length ? project.files.join(', ') : 'no definition files'}</p>
              </div>
              <button
                type="button"
                className={BTN}
                disabled={busy}
                onClick={() => void setConsent('project', project.path, false)}
              >
                Revoke consent
              </button>
            </li>
          ))}
        </ul>
        <h4 className="mb-1 mt-4 text-xs font-semibold">Extra directories</h4>
        <ul className="space-y-2">
          {(listing?.extra_dirs ?? []).map((dir) => (
            <li key={dir.path} data-testid="extra-dir-row" className="flex items-center gap-3 rounded-lg border border-pm-border p-3">
              <div className="min-w-0 flex-1">
                <p className="break-all text-xs">{dir.path}</p>
                {!dir.exists && <p className="text-xs text-pm-muted">This folder does not exist.</p>}
              </div>
              <Switch
                checked={dir.consent}
                label={`Consent to definitions in ${dir.path}`}
                disabled={busy}
                onToggle={() => void setConsent('extra', dir.path, !dir.consent)}
              />
              <button type="button" className={BTN} disabled={busy} onClick={() => void extraDir('remove', dir.path)}>
                Remove
              </button>
            </li>
          ))}
        </ul>
        <div className="mt-2 flex items-center gap-2">
          <input
            type="text"
            aria-label="Extra directory"
            value={extraPath}
            onChange={(e) => setExtraPath(e.target.value)}
            placeholder="D:\team-agents"
            className="min-w-0 flex-1 rounded-lg border border-pm-border bg-transparent px-2 py-1 text-xs"
          />
          <button
            type="button"
            className={BTN}
            disabled={busy || !extraPath.trim()}
            onClick={() => void extraDir('add', extraPath.trim())}
          >
            Add directory
          </button>
        </div>
        <p className="mt-2 text-xs text-pm-muted">
          A new directory loads nothing until you consent to it, and every file in it still needs your approval.
        </p>
      </SettingBlock>
      {wizardOpen && (
        <AgentWizard api={wizardApi} onClose={() => setWizardOpen(false)} onApprove={(done) => void wizardDone(done)} />
      )}
      {reviewing && (
        <ApproveDialog
          reviewing={reviewing}
          busy={busy}
          onApprove={() => void approve()}
          onCancel={() => setReviewing(null)}
        />
      )}
    </>
  );
}

function Switch({
  checked,
  label,
  disabled,
  onToggle,
}: {
  checked: boolean;
  label: string;
  disabled: boolean;
  onToggle: () => void;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={onToggle}
      className={['relative h-7 w-12 shrink-0 rounded-full transition', checked ? 'bg-pm-accent' : 'bg-pm-border'].join(' ')}
    >
      <span
        className={[
          'absolute top-1 h-5 w-5 rounded-full bg-pm-bg shadow-sm transition',
          checked ? 'left-6' : 'left-1',
        ].join(' ')}
      />
    </button>
  );
}

/** Inline (not modal) notice that the open project ships its own agent definitions. */
function ConsentBanner({
  scan,
  busy,
  onAllow,
  onDismiss,
}: {
  scan: ProjectScan;
  busy: boolean;
  onAllow: () => void;
  onDismiss: () => void;
}) {
  return (
    <div
      role="region"
      aria-label="Project agent definitions"
      data-testid="consent-banner"
      className="mb-4 rounded-lg border border-pm-border bg-pm-border/20 p-3"
    >
      <p className="text-sm font-semibold">This project contains agent definitions</p>
      <p className="mt-1 break-all text-xs text-pm-muted">{scan.project}</p>
      <ul className="mt-2 list-disc pl-5 text-xs">
        {scan.files.map((file) => (
          <li key={file}>{file}</li>
        ))}
      </ul>
      <p className="mt-2 text-xs text-pm-muted">
        A definition tells Puppet Master which program to run. Nothing is loaded until you allow this project, and
        each file still needs your approval. A copy of the project in another folder asks again.
      </p>
      <div className="mt-3 flex gap-2">
        <button type="button" className={BTN} disabled={busy} onClick={onAllow}>
          Allow for this project
        </button>
        <button type="button" className={BTN} onClick={onDismiss}>
          Not now
        </button>
      </div>
    </div>
  );
}

function noFixturesMessage(check: FixtureCheck): string {
  const name = check.name || 'this definition';
  if (check.reason === 'not_a_folder') {
    return `The fixtures path ${check.dir} is not a folder. Point the field at a folder of captures, or clear it to use the default.`;
  }
  const lead = check.explicit
    ? check.reason === 'empty'
      ? `The fixtures folder ${check.dir} has no captures.`
      : `The fixtures folder you entered does not exist: ${check.dir}.`
    : check.reason === 'empty'
      ? `The folder for ${name} exists but has no captures yet.`
      : `No captures yet for ${name}.`;
  const how = `Put captured screens or JSON output in ${check.dir} (each with a <name>.expect.toml), or use Rec on a pane to record a session.`;
  const optional = check.generic ? ' Captures are optional for experimental (generic) agents.' : '';
  return `${lead} ${how}${optional}`;
}

function DefinitionItem({
  row,
  busy,
  onReview,
  onRevoke,
  onCheck,
  onExport,
  check,
  onCreateFixtures,
}: {
  row: DefinitionRow;
  busy: boolean;
  onReview: (row: DefinitionRow) => void;
  onRevoke: (row: DefinitionRow) => void;
  onCheck: (row: DefinitionRow) => void;
  onExport: (row: DefinitionRow) => void;
  check: FixtureCheck | null;
  onCreateFixtures: (row: DefinitionRow) => void;
}) {
  return (
    <li data-testid="definition-row" className="rounded-lg border border-pm-border p-3">
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-sm font-semibold">{row.id ?? fileName(row.file)}</span>
        <span data-testid="source-badge" className={`${BADGE} border-pm-border text-pm-muted`}>
          {sourceLabel(row.source)}
        </span>
        <span
          data-testid="state-badge"
          className={`${BADGE} ${STATE_TONE[row.state] ?? 'border-pm-border text-pm-muted'}`}
        >
          {stateLabel(row.state)}
        </span>
        {row.experimental && (
          <span
            data-testid="experimental-badge"
            title="Experimental: runs a generic command with no verified behavior"
            className={`${BADGE} border-pm-border text-pm-muted`}
          >
            experimental
          </span>
        )}
        <span className="ml-auto flex gap-2">
          <button type="button" className={BTN} disabled={busy} onClick={() => onCheck(row)}>
            Check with fixtures
          </button>
          {row.source === 'user' && row.id && (
            <button type="button" className={BTN} disabled={busy} onClick={() => onExport(row)}>
              Export
            </button>
          )}
          {canReview(row) && (
            <button type="button" className={BTN} disabled={busy} onClick={() => onReview(row)}>
              Review and approve
            </button>
          )}
          {row.state === 'active' && row.id && (
            <button type="button" className={BTN} disabled={busy} onClick={() => onRevoke(row)}>
              Revoke
            </button>
          )}
        </span>
      </div>
      <p className="mt-1 break-all text-xs text-pm-muted">{row.file}</p>
      {check && check.status === 'no_fixtures' && (
        <div data-testid="fixtures-none" className="mt-2 text-xs text-pm-muted">
          <p>{noFixturesMessage(check)}</p>
          {check.reason !== 'not_a_folder' && (
            <button
              type="button"
              className={`${BTN} mt-2`}
              disabled={busy || check.reason === 'empty'}
              onClick={() => onCreateFixtures(row)}
            >
              {check.reason === 'empty' ? 'Folder exists' : 'Create folder'}
            </button>
          )}
        </div>
      )}
      {check && check.status === 'ran' && (
        <ul data-testid="fixture-results" className="mt-2 space-y-1">
          {check!.results.length === 0 && <li className="text-xs text-pm-muted">No fixtures found.</li>}
          {check!.results.map((r) => (
            <li key={r.fixture} data-testid="fixture-result" className="text-xs">
              <span className={r.passed ? 'text-pm-muted' : 'font-semibold text-pm-text'}>{r.passed ? 'PASS' : 'FAIL'}</span>{' '}
              <span className="font-mono">{r.fixture}</span>
              {r.failures.map((f, i) => (
                <div key={i} className="ml-6 text-pm-text">
                  {f}
                </div>
              ))}
            </li>
          ))}
        </ul>
      )}
      {row.shadowed_by && (
        <p className="mt-1 text-xs text-pm-muted">Shadowed by {fileName(row.shadowed_by)}</p>
      )}
      {row.errors.length > 0 && (
        <ul className="mt-2 space-y-1">
          {row.errors.map((e, i) => (
            <li key={i} className="text-xs text-pm-text">
              <span className="font-mono">{e.code}</span>{' '}
              <span className="font-mono">
                {fileName(row.file)}
                {e.line != null ? `:${e.line}${e.column != null ? `:${e.column}` : ''}` : ''}
              </span>{' '}
              {e.message}
            </li>
          ))}
        </ul>
      )}
    </li>
  );
}

function ApproveDialog({
  reviewing,
  busy,
  onApprove,
  onCancel,
}: {
  reviewing: Reviewing;
  busy: boolean;
  onApprove: () => void;
  onCancel: () => void;
}) {
  const { row, preview } = reviewing;
  const rendered = preview?.rendered;
  const blocked = !preview || !preview.ok;
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4">
      <div
        role="dialog"
        aria-modal="true"
        aria-label={`Approve ${row.id ?? fileName(row.file)}`}
        className="max-h-full w-full max-w-xl overflow-auto rounded-xl border border-pm-border bg-pm-panel p-5"
      >
        <h3 className="text-sm font-semibold">Approve {row.id ?? fileName(row.file)}</h3>
        <p className="mt-1 break-all text-xs text-pm-muted">{row.file}</p>
        {reviewing.error && <p className="mt-3 text-sm text-pm-text">{reviewing.error}</p>}
        {rendered && (
          <div className="mt-3 space-y-2">
            <ArgvBlock title="Headless command" argv={rendered.headless_argv} />
            <ArgvBlock title="Visible pane command" argv={rendered.tui_argv} />
          </div>
        )}
        <ChangesSinceApproval preview={preview} />
        <ul className="mt-3 list-disc space-y-1 pl-5 text-xs text-pm-muted">
          {approvalWarnings(row, preview).map((w) => (
            <li key={w}>{w}</li>
          ))}
        </ul>
        {preview?.errors.map((e, i) => (
          <p key={i} className="mt-1 text-xs text-pm-text">
            <span className="font-mono">{e.code}</span> {e.message}
          </p>
        ))}
        <div className="mt-4 flex justify-end gap-2">
          <button type="button" className={BTN} onClick={onCancel}>
            Cancel
          </button>
          <button type="button" className={BTN} disabled={busy || blocked} onClick={onApprove}>
            Approve
          </button>
        </div>
      </div>
    </div>
  );
}

function ChangesSinceApproval({ preview }: { preview: DefinitionPreview | null }) {
  const approved = preview?.approved;
  if (!preview || !approved) return null;
  const blocks = [
    { title: 'Headless command', before: approved.headless_argv, after: preview.rendered.headless_argv },
    { title: 'Visible pane command', before: approved.tui_argv, after: preview.rendered.tui_argv },
  ].filter((b) => b.before && b.after && b.before.join('\u0000') !== b.after.join('\u0000'));
  if (blocks.length === 0 && !preview.binary_changed) return null;
  return (
    <div data-testid="approval-diff" className="mt-3 rounded-lg border border-pm-border p-2">
      <div className="text-xs font-medium">Changed since you approved it</div>
      {blocks.map((block) => (
        <div key={block.title} className="mt-2">
          <div className="text-xs text-pm-muted">{block.title}</div>
          <pre className="mt-1 overflow-x-auto whitespace-pre-wrap break-all rounded-lg bg-pm-bg p-2 text-xs">
            {argvDiff(block.before ?? [], block.after ?? []).map((line, i) => (
              <div
                key={i}
                data-diff={line.kind}
                className={line.kind === 'added' ? 'font-medium text-pm-text' : line.kind === 'removed' ? 'text-pm-muted' : ''}
              >
                {line.kind === 'added' ? '+ ' : line.kind === 'removed' ? '- ' : '  '}
                {line.text}
              </div>
            ))}
          </pre>
        </div>
      ))}
      {preview.binary_changed && (
        <p className="mt-2 break-all text-xs text-pm-muted">Approved program: {approved.binary}</p>
      )}
    </div>
  );
}

function ArgvBlock({ title, argv }: { title: string; argv: string[] | null }) {
  return (
    <div>
      <div className="text-xs font-medium text-pm-muted">{title}</div>
      <pre className="mt-1 overflow-x-auto whitespace-pre-wrap break-all rounded-lg bg-pm-bg p-2 text-xs">
        {argv ? formatArgv(argv) : 'not available'}
      </pre>
    </div>
  );
}
