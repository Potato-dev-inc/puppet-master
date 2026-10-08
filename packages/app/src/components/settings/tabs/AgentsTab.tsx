// Settings > Agents: declarative agent definitions (list, errors, reload, approve/revoke).
// Every string from a definition file is rendered as React text, never as HTML (UI-4).
import { useCallback, useEffect, useState } from 'react';
import {
  approvalWarnings,
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
  type FixtureResult,
} from '../../../lib/agentDefinitions';
import { resetAgentRegistryCache } from '../../../lib/agentRegistry';
import { SettingBlock, SettingsSection } from '../settings-ui';

const STATE_TONE: Record<string, string> = {
  active: 'border-emerald-500/40 bg-emerald-500/10 text-emerald-400',
  unapproved: 'border-amber-500/40 bg-amber-500/10 text-amber-400',
  invalid: 'border-red-500/40 bg-red-500/10 text-red-400',
};

const BADGE = 'inline-flex shrink-0 items-center rounded-full border px-2 py-0.5 text-[10px] font-medium';
const BTN =
  'rounded-lg border border-pm-border px-3 py-1.5 text-xs font-medium hover:bg-pm-border/40 disabled:opacity-50';

interface Reviewing {
  row: DefinitionRow;
  preview: DefinitionPreview | null;
  error: string | null;
}

export function AgentsTab({ api = tauriAgentDefinitionsApi }: { api?: AgentDefinitionsApi }) {
  const [listing, setListing] = useState<DefinitionsListing | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [reviewing, setReviewing] = useState<Reviewing | null>(null);
  const [fixturesDir, setFixturesDir] = useState('');
  const [genericCommand, setGenericCommand] = useState('');
  const [checks, setChecks] = useState<{ file: string; results: FixtureResult[] } | null>(null);
  const [drafts, setDrafts] = useState<DraftsListing | null>(null);
  const [draftCheck, setDraftCheck] = useState<{ path: string; preview: DefinitionPreview } | null>(null);

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

  useEffect(() => {
    void load();
    void loadDrafts();
  }, [load, loadDrafts]);

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

  const check = (row: DefinitionRow) =>
    run(async () => {
      setChecks({ file: row.file, results: await api.checkFile(row.file, fixturesDir.trim() || undefined) });
    });

  const validateDraft = (path: string) =>
    run(async () => {
      if (!api.draftValidate) return;
      setDraftCheck({ path, preview: await api.draftValidate(path) });
    });

  const review = (row: DefinitionRow) =>
    run(async () => {
      try {
        setReviewing({ row, preview: await api.preview(row.file), error: null });
      } catch (e) {
        setReviewing({ row, preview: null, error: errorMessage(e) });
      }
    });

  const approve = () =>
    run(async () => {
      if (!reviewing?.row.fingerprint) return;
      await api.approve(reviewing.row.file, reviewing.row.fingerprint);
      setReviewing(null);
      apply(await api.list());
    });

  const setConsent = (kind: string, path: string, enabled: boolean) =>
    run(async () => {
      await api.consent(kind, path, enabled);
      apply(await api.list());
    });

  const revoke = (row: DefinitionRow) =>
    run(async () => {
      if (!row.id) return;
      await api.revoke(row.id);
      apply(await api.list());
    });

  return (
    <SettingsSection
      title="Agents"
      description="Agents defined by TOML files. A definition runs a program, so a new or changed one stays inactive until you review and approve it here."
    >
      <SettingBlock label="Definitions" implemented>
        <div className="mb-3 flex items-center gap-2">
          <button type="button" className={BTN} disabled={busy} onClick={() => void reload()}>
            Reload
          </button>
          {note && <span className="text-xs text-pm-muted">{note}</span>}
        </div>
        {error && (
          <p role="alert" className="mb-3 text-sm text-red-400">
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
              results={checks?.file === row.file ? checks.results : null}
            />
          ))}
        </ul>
      </SettingBlock>
      {api.draftsList && (
        <SettingBlock
          label="Drafts"
          description="Definitions drafted from a recorded pane session (the Rec button in a pane header). A draft is never loaded or approved: review it, copy it to the agents folder, then approve it above."
          implemented
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
                    <p className={draftCheck.preview.ok ? 'text-emerald-400' : 'text-red-400'}>
                      {draftCheck.preview.ok ? 'Draft is valid.' : 'Draft has errors:'}
                    </p>
                    {draftCheck.preview.errors.map((err, i) => (
                      <p key={i} className="text-red-400">
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
        implemented
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
              {source.consent !== null && (
                <button
                  type="button"
                  role="switch"
                  aria-checked={source.consent}
                  aria-label={`Consent to ${sourceLabel(source.name)} definitions`}
                  disabled={busy}
                  onClick={() => void setConsent(source.name, source.path ?? '', !source.consent)}
                  className={[
                    'relative h-7 w-12 shrink-0 rounded-full transition',
                    source.consent ? 'bg-pm-accent' : 'bg-pm-border',
                  ].join(' ')}
                >
                  <span
                    className={[
                      'absolute top-1 h-5 w-5 rounded-full bg-pm-bg shadow-sm transition',
                      source.consent ? 'left-6' : 'left-1',
                    ].join(' ')}
                  />
                </button>
              )}
            </li>
          ))}
        </ul>
      </SettingBlock>
      {reviewing && (
        <ApproveDialog
          reviewing={reviewing}
          busy={busy}
          onApprove={() => void approve()}
          onCancel={() => setReviewing(null)}
        />
      )}
    </SettingsSection>
  );
}

function DefinitionItem({
  row,
  busy,
  onReview,
  onRevoke,
  onCheck,
  results,
}: {
  row: DefinitionRow;
  busy: boolean;
  onReview: (row: DefinitionRow) => void;
  onRevoke: (row: DefinitionRow) => void;
  onCheck: (row: DefinitionRow) => void;
  results: FixtureResult[] | null;
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
        <span className="ml-auto flex gap-2">
          <button type="button" className={BTN} disabled={busy} onClick={() => onCheck(row)}>
            Check with fixtures
          </button>
          {row.state === 'unapproved' && row.fingerprint && (
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
      {results && (
        <ul data-testid="fixture-results" className="mt-2 space-y-1">
          {results.length === 0 && <li className="text-xs text-pm-muted">No fixtures found.</li>}
          {results.map((r) => (
            <li key={r.fixture} data-testid="fixture-result" className="text-xs">
              <span className={r.passed ? 'text-emerald-400' : 'text-red-400'}>{r.passed ? 'PASS' : 'FAIL'}</span>{' '}
              <span className="font-mono">{r.fixture}</span>
              {r.failures.map((f, i) => (
                <div key={i} className="ml-6 text-red-400">
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
            <li key={i} className="text-xs text-red-400">
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
        {reviewing.error && <p className="mt-3 text-sm text-red-400">{reviewing.error}</p>}
        {rendered && (
          <div className="mt-3 space-y-2">
            <ArgvBlock title="Headless command" argv={rendered.headless_argv} />
            <ArgvBlock title="Visible pane command" argv={rendered.tui_argv} />
          </div>
        )}
        <ul className="mt-3 list-disc space-y-1 pl-5 text-xs text-amber-400">
          {approvalWarnings(row, preview).map((w) => (
            <li key={w}>{w}</li>
          ))}
        </ul>
        {preview?.errors.map((e, i) => (
          <p key={i} className="mt-1 text-xs text-red-400">
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
