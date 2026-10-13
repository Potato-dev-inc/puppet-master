// Settings > MCP & bridge > Toolbelt: list MCP servers once, apply them to every agent.
import { useCallback, useEffect, useState } from 'react';
import {
  errText,
  joinArgs,
  pairsText,
  parsePairs,
  splitArgs,
  toolbeltApi,
  type ToolbeltApi,
  type ToolbeltListing,
  type ToolServer,
} from '../../../lib/toolbelt';
import { ToolbeltImport } from './ToolbeltImport';
import { ToolbeltPreview } from './ToolbeltPreview';
import { btn, btnPrimary, field } from './styles';

interface Draft {
  id: string;
  name: string;
  transport: 'stdio' | 'http';
  command: string;
  args: string;
  env: string;
  url: string;
  headers: string;
  scope: 'global' | 'project';
  enabled: boolean;
}

const EMPTY: Draft = {
  id: '',
  name: '',
  transport: 'stdio',
  command: '',
  args: '',
  env: '',
  url: '',
  headers: '',
  scope: 'global',
  enabled: true,
};

function toDraft(s: ToolServer): Draft {
  return {
    id: s.id,
    name: s.name,
    transport: s.transport,
    command: s.command ?? '',
    args: joinArgs(s.args),
    env: pairsText(s.env),
    url: s.url ?? '',
    headers: pairsText(s.headers, ':'),
    scope: s.scope,
    enabled: s.enabled,
  };
}

function toServer(d: Draft): ToolServer {
  return d.transport === 'stdio'
    ? { id: d.id, name: d.name, transport: 'stdio', command: d.command, args: splitArgs(d.args), env: parsePairs(d.env), enabled: d.enabled, scope: d.scope }
    : { id: d.id, name: d.name, transport: 'http', url: d.url, headers: parsePairs(d.headers, ':'), enabled: d.enabled, scope: d.scope };
}

function summary(s: ToolServer): string {
  return s.transport === 'http' ? (s.url ?? '') : [s.command, ...(s.args ?? [])].join(' ');
}

function Switch({ on, label, onChange }: { on: boolean; label: string; onChange: (next: boolean) => void }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      aria-label={label}
      onClick={() => onChange(!on)}
      className={`relative h-5 w-9 shrink-0 rounded-full border border-pm-border transition ${on ? 'bg-pm-text' : 'bg-pm-bg'}`}
    >
      <span
        className={`absolute top-0.5 h-3.5 w-3.5 rounded-full transition-all ${on ? 'left-[18px] bg-pm-bg' : 'left-0.5 bg-pm-muted'}`}
      />
    </button>
  );
}

export function ToolbeltGroup({ api = toolbeltApi, projectPath }: { api?: ToolbeltApi; projectPath?: string | null }) {
  const [listing, setListing] = useState<ToolbeltListing | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [saving, setSaving] = useState(false);
  const [previewing, setPreviewing] = useState(false);
  const [importing, setImporting] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setListing(await api.list());
      setError(null);
    } catch (e) {
      setError(errText(e));
    }
  }, [api]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const save = async () => {
    if (!draft || saving) return;
    setSaving(true);
    setError(null);
    try {
      const server = toServer(draft);
      if (draft.id) await api.update(draft.id, server);
      else await api.add(server);
      setDraft(null);
      await refresh();
    } catch (e) {
      setError(errText(e));
    } finally {
      setSaving(false);
    }
  };

  const toggle = async (s: ToolServer, enabled: boolean) => {
    try {
      await api.update(s.id, { ...s, enabled });
      await refresh();
    } catch (e) {
      setError(errText(e));
    }
  };

  const remove = async (s: ToolServer) => {
    try {
      await api.remove(s.id);
      await refresh();
    } catch (e) {
      setError(errText(e));
    }
  };

  const servers = listing?.servers ?? [];

  return (
    <section aria-label="Toolbelt" className="rounded-xl border border-pm-border bg-pm-panel p-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <h3 className="text-sm font-semibold">Toolbelt</h3>
          <p className="mt-1 max-w-xl text-sm leading-6 text-pm-muted">
            Add an MCP server once. Puppet Master writes it into every agent&apos;s own config, so each agent can use it.
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <button type="button" className={btn} onClick={() => setImporting(true)}>
            Import from agents
          </button>
          <button type="button" className={btn} onClick={() => setDraft({ ...EMPTY })}>
            Add server
          </button>
          <button type="button" className={btnPrimary} onClick={() => setPreviewing(true)} disabled={!listing}>
            Apply to all agents
          </button>
        </div>
      </div>

      <p className="mt-3 text-xs leading-5 text-pm-muted">
        Keys, tokens and passwords are kept in your system's secure storage and hidden here. Each agent&apos;s config file needs the real
        value, so Puppet Master writes it there as plain text.
      </p>

      {error && (
        <p role="alert" className="mt-3 rounded-lg border border-pm-border bg-pm-bg px-3 py-2 text-xs leading-5 text-pm-text">
          {error}
        </p>
      )}
      {notice && <p className="mt-3 text-xs leading-5 text-pm-muted">{notice}</p>}

      <ul className="mt-3 divide-y divide-pm-border overflow-hidden rounded-lg border border-pm-border" aria-label="Servers">
        {servers.map((s) => (
          <li key={s.id} className="flex items-center gap-3 bg-pm-bg px-3 py-2.5">
            <Switch on={s.enabled} label={`Use ${s.name}`} onChange={(next) => void toggle(s, next)} />
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2">
                <span className="truncate text-sm font-medium">{s.name}</span>
                <span className="shrink-0 text-[11px] text-pm-muted">{s.scope === 'project' ? 'This project' : 'All projects'}</span>
              </div>
              <div className="truncate font-mono text-[11px] text-pm-muted">{summary(s)}</div>
            </div>
            <button type="button" className={btn} onClick={() => setDraft(toDraft(s))} aria-label={`Edit ${s.name}`}>
              Edit
            </button>
            <button type="button" className={btn} onClick={() => void remove(s)} aria-label={`Remove ${s.name}`}>
              Remove
            </button>
          </li>
        ))}
        {listing && servers.length === 0 && (
          <li className="bg-pm-bg px-3 py-4 text-sm text-pm-muted">
            No servers yet. Add one, or import the ones your agents already use.
          </li>
        )}
      </ul>

      {listing && (
        <p className="mt-3 text-xs leading-5 text-pm-muted">
          Works with {listing.agents.map((a) => a.label).join(', ') || 'no agents yet'}.
          {listing.unsupported.length > 0 && ` Not supported: ${listing.unsupported.map((a) => a.label).join(', ')}.`}
        </p>
      )}

      {draft && (
        <form
          aria-label={draft.id ? 'Edit server' : 'Add server'}
          className="mt-3 space-y-3 rounded-lg border border-pm-border bg-pm-bg p-3"
          onSubmit={(e) => {
            e.preventDefault();
            void save();
          }}
        >
          <label className="block text-xs text-pm-muted">
            Name
            <input className={field} value={draft.name} onChange={(e) => setDraft({ ...draft, name: e.target.value })} placeholder="Browser" />
          </label>
          <div role="group" aria-label="Type" className="inline-flex rounded-lg border border-pm-border p-0.5 text-xs">
            {(['stdio', 'http'] as const).map((t) => (
              <button
                key={t}
                type="button"
                aria-pressed={draft.transport === t}
                onClick={() => setDraft({ ...draft, transport: t })}
                className={`rounded-md px-3 py-1 ${draft.transport === t ? 'bg-pm-text text-pm-bg' : 'text-pm-muted'}`}
              >
                {t === 'stdio' ? 'Command' : 'Web address'}
              </button>
            ))}
          </div>
          {draft.transport === 'stdio' ? (
            <>
              <label className="block text-xs text-pm-muted">
                Command
                <input className={`${field} font-mono`} value={draft.command} onChange={(e) => setDraft({ ...draft, command: e.target.value })} placeholder="npx" />
              </label>
              <label className="block text-xs text-pm-muted">
                Arguments
                <input className={`${field} font-mono`} value={draft.args} onChange={(e) => setDraft({ ...draft, args: e.target.value })} placeholder="-y @playwright/mcp" />
              </label>
              <label className="block text-xs text-pm-muted">
                Environment variables, one per line (NAME=value)
                <textarea className={`${field} font-mono`} rows={2} value={draft.env} onChange={(e) => setDraft({ ...draft, env: e.target.value })} />
              </label>
            </>
          ) : (
            <>
              <label className="block text-xs text-pm-muted">
                Address
                <input className={`${field} font-mono`} value={draft.url} onChange={(e) => setDraft({ ...draft, url: e.target.value })} placeholder="https://example.com/mcp" />
              </label>
              <label className="block text-xs text-pm-muted">
                Headers, one per line (Name: value)
                <textarea className={`${field} font-mono`} rows={2} value={draft.headers} onChange={(e) => setDraft({ ...draft, headers: e.target.value })} />
              </label>
            </>
          )}
          <label className="block text-xs text-pm-muted">
            Use in
            <select className={field} value={draft.scope} onChange={(e) => setDraft({ ...draft, scope: e.target.value as Draft['scope'] })}>
              <option value="global">All projects</option>
              <option value="project">This project only</option>
            </select>
          </label>
          <div className="flex justify-end gap-2">
            <button type="button" className={btn} onClick={() => setDraft(null)}>
              Cancel
            </button>
            <button type="submit" className={btnPrimary} disabled={saving}>
              {saving ? 'Saving…' : 'Save'}
            </button>
          </div>
        </form>
      )}

      {previewing && (
        <ToolbeltPreview
          api={api}
          projectPath={projectPath ?? null}
          onClose={() => setPreviewing(false)}
          onApplied={(text) => {
            setPreviewing(false);
            setNotice(text);
          }}
        />
      )}
      {importing && (
        <ToolbeltImport
          api={api}
          projectPath={projectPath ?? null}
          onClose={() => setImporting(false)}
          onImported={(count) => {
            setImporting(false);
            setNotice(count === 1 ? 'Added 1 server to your toolbelt.' : `Added ${count} servers to your toolbelt.`);
            void refresh();
          }}
        />
      )}
    </section>
  );
}
