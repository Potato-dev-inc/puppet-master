// Body of the cockpit "+" menu: pick an agent; an agent that has user accounts gets a second step
// to pick which login to start it under ("Default login" is the CLI's own).
import { useEffect, useState } from 'react';
import { accountsFor, needsAccountStep, tauriAccountsApi, type Account, type AccountsApi } from '../../lib/accounts';

export interface LaunchEntry {
  type: string;
  label: string;
  disabled?: boolean;
  disabledReason?: string;
}

interface Props {
  entries: LaunchEntry[];
  readyAgents: Set<string>;
  onStart: (agentId: string, accountId?: string) => void;
  /** Called after an agent was started, so the host closes the menu. */
  onClose: () => void;
  accountsApi?: AccountsApi;
}

export function NewAgentMenu({ entries, readyAgents, onStart, onClose, accountsApi = tauriAccountsApi }: Props) {
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [picking, setPicking] = useState<LaunchEntry | null>(null);

  useEffect(() => {
    let alive = true;
    accountsApi
      .list()
      .then((o) => alive && setAccounts(o.accounts))
      .catch(() => undefined); // no accounts store (browser, old backend): the plain menu still works
    return () => {
      alive = false;
    };
  }, [accountsApi]);

  const start = (agentId: string, accountId?: string) => {
    onClose();
    onStart(agentId, accountId);
  };

  if (picking) {
    return (
      <>
        <div className="ck-menu-cap">{picking.label} · login</div>
        <button type="button" role="menuitem" className="ck-menu-item" onClick={() => start(picking.type)}>
          <span>Default login</span>
        </button>
        {accountsFor(accounts, picking.type).map((a) => (
          <button key={a.id} type="button" role="menuitem" className="ck-menu-item" onClick={() => start(picking.type, a.id)}>
            <span>{a.label}</span>
          </button>
        ))}
        <button type="button" role="menuitem" className="ck-menu-item" onClick={() => setPicking(null)}>
          <span>Back</span>
        </button>
      </>
    );
  }

  return (
    <>
      <div className="ck-menu-cap">New agent</div>
      {entries.map((e) => {
        const hasAccounts = needsAccountStep(accounts, e.type);
        return (
          <button
            key={e.type}
            type="button"
            role="menuitem"
            className="ck-menu-item"
            disabled={e.disabled}
            title={e.disabledReason}
            onClick={() => (hasAccounts ? setPicking(e) : start(e.type))}
          >
            <span>{e.label}</span>
            {!e.disabled && hasAccounts && <span className="ck-menu-hint">{accountsFor(accounts, e.type).length + 1} logins ›</span>}
            {!e.disabled && !hasAccounts && readyAgents.has(e.type) && <span className="ck-menu-hint">ready</span>}
            {e.disabled && <span className="ck-menu-hint">headless</span>}
          </button>
        );
      })}
    </>
  );
}
