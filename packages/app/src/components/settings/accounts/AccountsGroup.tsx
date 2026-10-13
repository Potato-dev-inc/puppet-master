// Settings > Agents > Accounts: run one agent under several logins. Per agent that supports
// accounts: its accounts, "Add account", and "Open login pane" (starts the agent under that
// account so the user logs in once). Puppet Master only points the CLI at a folder the user
// controls; it never reads or shares the credentials inside.
import { useCallback, useEffect, useState } from 'react';
import {
  accountsFor,
  errorText,
  tauriAccountsApi,
  type AccountsApi,
  type AccountsOverview,
} from '../../../lib/accounts';
import { agentLabel, useAgentRegistry } from '../../../lib/agentRegistry';
import { SettingBlock } from '../settings-ui';

const BTN = 'rounded-lg border border-pm-border px-3 py-1.5 text-xs font-medium hover:bg-pm-border/40 disabled:opacity-50';

export function AccountsGroup({ api = tauriAccountsApi }: { api?: AccountsApi }) {
  const [data, setData] = useState<AccountsOverview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  const registry = useAgentRegistry();

  const refresh = useCallback(async () => {
    try {
      setData(await api.list());
    } catch {
      setData(null); // no desktop shell / old backend: nothing to configure
    }
  }, [api]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const run = async (work: () => Promise<unknown>, done?: string) => {
    setBusy(true);
    setError(null);
    setNote(null);
    try {
      await work();
      if (done) setNote(done);
      await refresh();
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  if (!data || data.agents.length === 0) return null;

  return (
    <SettingBlock
      label="Accounts"
      description="Use the same agent with more than one login, and pick one per pane. Each account is a folder you control; Puppet Master only points the agent at it and never reads or shares what is inside. Check your provider's terms before using several accounts."
    >
      {error && (
        <p role="alert" className="mb-3 text-sm text-pm-muted">
          {error}
        </p>
      )}
      {note && (
        <p role="status" className="mb-3 text-xs text-pm-muted">
          {note}
        </p>
      )}
      <div className="space-y-4">
        {data.agents.map((agent) => {
          const mine = accountsFor(data.accounts, agent.agent_id);
          const draft = drafts[agent.agent_id] ?? '';
          return (
            <section key={agent.agent_id} aria-label={`${agent.agent_id} accounts`} data-testid={`accounts-${agent.agent_id}`}>
              <h4 className="text-sm font-medium">{agentLabel(registry, agent.agent_id)}</h4>
              <p className="mt-0.5 text-xs text-pm-muted">{agent.login_hint}</p>
              {mine.length === 0 && <p className="mt-2 text-xs text-pm-muted">No accounts yet. It uses its own login.</p>}
              <ul className="mt-2 divide-y divide-pm-border rounded-lg border border-pm-border">
                {mine.map((a) => (
                  <li key={a.id} className="flex items-center gap-2 px-3 py-2">
                    <input
                      aria-label={`Name of ${a.label}`}
                      defaultValue={a.label}
                      className="min-w-0 flex-1 bg-transparent text-sm outline-none"
                      onBlur={(e) => {
                        const label = e.target.value.trim();
                        if (label && label !== a.label) void run(() => api.rename(a.id, label));
                        else e.target.value = a.label;
                      }}
                    />
                    <button
                      type="button"
                      className={BTN}
                      disabled={busy}
                      onClick={() => void run(() => api.openLoginPane(agent.agent_id, a.id), 'Opened a pane. Log in there once.')}
                    >
                      Open login pane
                    </button>
                    <button
                      type="button"
                      className={BTN}
                      disabled={busy}
                      title="Forgets the account. Its folder is kept."
                      onClick={() => void run(() => api.remove(a.id, false))}
                    >
                      Remove
                    </button>
                  </li>
                ))}
              </ul>
              <form
                className="mt-2 flex items-center gap-2"
                onSubmit={(e) => {
                  e.preventDefault();
                  const label = draft.trim();
                  if (!label) return;
                  void run(async () => {
                    const created = await api.add(agent.agent_id, label);
                    setDrafts((d) => ({ ...d, [agent.agent_id]: '' }));
                    return created;
                  });
                }}
              >
                <input
                  aria-label={`New ${agent.agent_id} account name`}
                  placeholder="Account name, for example Work"
                  value={draft}
                  maxLength={40}
                  onChange={(e) => setDrafts((d) => ({ ...d, [agent.agent_id]: e.target.value }))}
                  className="min-w-0 flex-1 rounded-lg border border-pm-border bg-pm-bg px-3 py-1.5 text-sm outline-none"
                />
                <button type="submit" className={BTN} disabled={busy || !draft.trim()}>
                  Add account
                </button>
              </form>
            </section>
          );
        })}
      </div>
    </SettingBlock>
  );
}
