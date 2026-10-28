// Settings > Agents > Keep going: the agents a task can move to when the agent it runs on runs out of usage,
// in order, and whether to ask to move it back after the reset. Saved through the keep-going settings command
// (not settings.json). Agent names come from the registry and the saved logins; nothing here names an agent.
import { useEffect, useState } from 'react';
import { accountsFor, errorText, tauriAccountsApi, type Account } from '../../../lib/accounts';
import { useAgentRegistry, type AgentDescriptor } from '../../../lib/agentRegistry';
import {
  keepGoingSettingsGet,
  keepGoingSettingsSet,
  keepGoingTargetKey,
  type KeepGoingSettings,
  type KeepGoingTarget,
} from '../../../lib/keep-going';
import { Popup } from '../controls';
import { Btn, Note, SettingBlock, SettingToggle } from '../settings-ui';

/** The most agents the list holds (the Rust `SETTINGS_ORDER_CAP`). */
export const KEEP_GOING_ORDER_CAP = 8;

/** One agent, or one saved login of an agent, that can be added to the list. */
export interface KeepGoingChoice {
  key: string;
  target: KeepGoingTarget;
  label: string;
}

/**
 * Every installed agent, and each of its saved logins, that is not listed yet, in registry order. An agent
 * that cannot run here is left out: the list only offers what the person can actually start.
 */
export function keepGoingChoices(agents: AgentDescriptor[], accounts: Account[], order: KeepGoingTarget[]): KeepGoingChoice[] {
  const listed = new Set(order.map(keepGoingTargetKey));
  const choices: KeepGoingChoice[] = [];
  for (const agent of agents) {
    if (!agent.installed || !agent.dispatchable) continue;
    const own: KeepGoingChoice[] = [{ key: agent.id, target: { agent_id: agent.id }, label: agent.label }];
    for (const account of accountsFor(accounts, agent.id)) {
      const target: KeepGoingTarget = { agent_id: agent.id, account: account.id };
      own.push({ key: keepGoingTargetKey(target), target, label: `${agent.label} · ${account.label}` });
    }
    choices.push(...own.filter((choice) => !listed.has(choice.key)));
  }
  return choices;
}

/** The name of one listed target: the agent, and its login when it has one. */
export function keepGoingLabel(target: KeepGoingTarget, agents: AgentDescriptor[], accounts: Account[]): string {
  const base = agents.find((agent) => agent.id === target.agent_id)?.label ?? target.agent_id;
  if (!target.account) return base;
  const account = accounts.find((item) => item.id === target.account);
  return account ? `${base} · ${account.label}` : `${base} · login removed`;
}

/** The text a failed save shows: the server's plain sentence, without its `CODE:` prefix. */
export function keepGoingSaveError(err: unknown): string {
  const text = errorText(err).replace(/^[A-Z][A-Z0-9_]*:\s*/, '').trim();
  return text || "Couldn't save. Try again.";
}

const HEADING = {
  label: 'When an agent runs out of usage',
  description: 'Puppet Master offers these agents, in this order, when the agent a task runs on hits its usage limit. You pick one each time.',
};

export function KeepGoingRows({ onSaved }: { onSaved?: () => void }) {
  const registry = useAgentRegistry();
  // undefined = still loading, null = not available here (a browser preview), otherwise the saved settings.
  const [settings, setSettings] = useState<KeepGoingSettings | null | undefined>(undefined);
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [pick, setPick] = useState('');
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    keepGoingSettingsGet()
      .then((saved) => alive && setSettings(saved))
      .catch(() => alive && setSettings(null));
    tauriAccountsApi
      .list()
      .then((overview) => alive && setAccounts(overview.accounts))
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, []);

  if (settings === undefined) return <SettingBlock {...HEADING} />;
  if (settings === null) {
    return (
      <SettingBlock {...HEADING}>
        <p className="text-sm text-pm-muted">Available in the desktop app.</p>
      </SettingBlock>
    );
  }

  const save = async (next: KeepGoingSettings) => {
    setError(null);
    try {
      setSettings(await keepGoingSettingsSet(next));
      onSaved?.();
    } catch (e) {
      setError(keepGoingSaveError(e));
    }
  };
  const move = (index: number, to: number) => {
    const order = [...settings.order];
    [order[index], order[to]] = [order[to], order[index]];
    void save({ ...settings, order });
  };
  const remove = (index: number) => void save({ ...settings, order: settings.order.filter((_, i) => i !== index) });

  const choices = keepGoingChoices(registry.agents, accounts, settings.order);
  const full = settings.order.length >= KEEP_GOING_ORDER_CAP;
  const chosen = choices.find((choice) => choice.key === pick);
  const add = () => {
    if (!chosen || full) return;
    setPick('');
    void save({ ...settings, order: [...settings.order, chosen.target] });
  };
  const last = settings.order.length - 1;

  return (
    <>
      <SettingBlock {...HEADING}>
        <div className="space-y-3">
          <p className="text-sm font-medium">Offer to continue on, in order</p>
          {settings.order.length === 0 ? (
            <p className="text-sm text-pm-muted">Nothing listed. Puppet Master then offers every other installed agent, in its usual order.</p>
          ) : (
            <ol className="space-y-2 text-sm" aria-label="Offer order">
              {settings.order.map((target, i) => {
                const key = keepGoingTargetKey(target);
                const label = keepGoingLabel(target, registry.agents, accounts);
                const installed = registry.agents.find((agent) => agent.id === target.agent_id)?.installed ?? false;
                return (
                  <li key={key} data-target-key={key} className="flex flex-wrap items-center gap-x-3 gap-y-1 rounded-lg border border-pm-border px-3 py-2">
                    <span className="w-4 font-mono text-xs text-pm-muted">{i + 1}</span>
                    <span className="font-medium">{label}</span>
                    <span className="text-xs text-pm-muted">{installed ? 'Installed' : 'Not installed'}</span>
                    <span className="ml-auto flex gap-1">
                      <Btn small aria-label={`Move ${label} up`} disabled={i === 0} onClick={() => move(i, i - 1)}>
                        ↑
                      </Btn>
                      <Btn small aria-label={`Move ${label} down`} disabled={i === last} onClick={() => move(i, i + 1)}>
                        ↓
                      </Btn>
                      <Btn small aria-label={`Remove ${label}`} onClick={() => remove(i)}>
                        Remove
                      </Btn>
                    </span>
                  </li>
                );
              })}
            </ol>
          )}
          <div className="flex flex-wrap items-center gap-2">
            <Popup
              aria-label="Agent to add"
              value={chosen ? pick : ''}
              disabled={full || choices.length === 0}
              onChange={(event) => setPick(event.target.value)}
            >
              <option value="">{choices.length > 0 ? 'Choose an agent' : 'No more agents to add'}</option>
              {choices.map((choice) => (
                <option key={choice.key} value={choice.key}>
                  {choice.label}
                </option>
              ))}
            </Popup>
            <Btn small disabled={!chosen || full} onClick={add}>
              Add
            </Btn>
            {full && <span className="text-xs text-pm-muted">You can list up to {KEEP_GOING_ORDER_CAP} agents.</span>}
          </div>
          <p className="text-xs text-pm-muted">Puppet Master asks every time. It never moves a task to another login of the same company.</p>
          {error && <Note role="alert">{error}</Note>}
        </div>
      </SettingBlock>
      <SettingToggle
        label="Ask to move it back when the first agent resets"
        description="Asks once, after the reset time. Never moves a task back on its own."
        checked={settings.ask_back}
        onChange={(on) => void save({ ...settings, ask_back: on })}
      />
    </>
  );
}
