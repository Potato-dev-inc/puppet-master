// User accounts per agent (Settings > Agents > Accounts, cockpit "+" menu). Backend: `accounts.rs`.
import { invoke } from '@tauri-apps/api/core';

export interface Account {
  id: string;
  agent_id: string;
  label: string;
  home: string;
}

/** An agent whose profile declares how it is pointed at another account. */
export interface AccountAgent {
  agent_id: string;
  login_hint: string;
  home_env: string;
}

export interface AccountsOverview {
  agents: AccountAgent[];
  accounts: Account[];
}

export interface AccountsApi {
  list: () => Promise<AccountsOverview>;
  add: (agentId: string, label: string) => Promise<Account>;
  rename: (id: string, label: string) => Promise<Account>;
  remove: (id: string, deleteHome?: boolean) => Promise<boolean>;
  /** Start the agent under the account in a new pane (the one-time login). */
  openLoginPane: (agentId: string, accountId: string) => Promise<string>;
}

const isTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) throw new Error(`Desktop shell required: ${command}`);
  return invoke<T>(command, args);
}

export const tauriAccountsApi: AccountsApi = {
  list: () => call<AccountsOverview>('accounts_list'),
  add: (agentId, label) => call<Account>('accounts_add', { agentId, label }),
  rename: (id, label) => call<Account>('accounts_rename', { id, label }),
  remove: (id, deleteHome = false) => call<boolean>('accounts_remove', { id, deleteHome }),
  openLoginPane: (agentId, accountId) => call<string>('new_pane', { agentId, accountId }),
};

export function accountsFor(accounts: Account[], agentId: string): Account[] {
  return accounts.filter((a) => a.agent_id === agentId);
}

/** The cockpit "+" menu asks for an account only when the agent has one to choose. */
export function needsAccountStep(accounts: Account[], agentId: string): boolean {
  return accountsFor(accounts, agentId).length > 0;
}

export function errorText(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}
