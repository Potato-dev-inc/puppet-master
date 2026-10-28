// Agent Store client: the `agent_store_*` Tauri commands (src-tauri/src/agent_store).
// Tauri commands only: there is no bridge route and no MCP tool, so an agent or a phone can never add or install an agent.
// Install and uninstall runs reuse the `agents_install_*` commands (see ./agent-install.ts).
import { invoke } from '@tauri-apps/api/core';
import type { InstallPlan, InstallStarted, InstallStep } from './agent-install';

export type StoreTier = 'verified' | 'starter';
export type StoreStatus = 'active' | 'deprecated';
export type StoreCategory = 'general' | 'open_source' | 'cloud_account' | 'bring_your_key' | 'local_models';
export type LicenseKind = 'open_source' | 'source_available' | 'proprietary';
export type SignInKind = 'account' | 'api_key' | 'either' | 'none';
export type CheckHow = 'ran_on_windows' | 'package_registry' | 'vendor_docs';

/** The card state from the Store's state table (src-tauri/src/agent_store/state.rs). */
export type StoreState =
  | 'available'
  | 'found_on_pc'
  | 'ready'
  | 'added_no_program'
  | 'needs_review'
  | 'own_version'
  | 'unsupported';

export interface StoreVendor {
  name: string;
  homepage: string;
  license: string;
  license_kind: LicenseKind;
  license_url: string | null;
  terms_url: string | null;
}

export interface StoreSignIn {
  kind: SignInKind;
  /** One plain sentence. Shown as text, never as HTML. */
  summary: string;
}

export interface StoreChecked {
  date: string;
  version: string;
  how: CheckHow;
  windows_tested: boolean;
}

export interface StoreLastCheck {
  passed: number;
  total: number;
  version: string;
  at: string;
}

/** One grid card. Field names match the JSON the Tauri command sends. */
export interface StoreCard {
  id: string;
  label: string;
  icon: string;
  tagline: string;
  tier: StoreTier;
  builtin: boolean;
  categories: StoreCategory[];
  status: StoreStatus;
  replaced_by: string;
  vendor: StoreVendor;
  sign_in: StoreSignIn;
  checked: StoreChecked;
  state: StoreState;
  /** Plain reason for the state, shown under the card. */
  state_reason: string;
  installed_version: string | null;
  /** e.g. "Pane", "One task". */
  modes: string[];
  can_install: boolean;
  ambiguous_name: boolean;
  last_check: StoreLastCheck | null;
}

export interface StoreList {
  catalog_version: string;
  entries: StoreCard[];
}

export interface StoreUninstall {
  available: boolean;
  steps: InstallStep[];
  /** The vendor's removal sentence for a script install, or "". */
  note: string;
}

/** The detail sheet: the card plus the install plan, the run command and the uninstall summary. */
export interface StoreDetail extends StoreCard {
  install_plan: InstallPlan;
  /** The argv Puppet Master runs for this agent, or null when there is no definition. */
  run_preview: string[] | null;
  fixtures: { count: number };
  uninstall: StoreUninstall;
  /** The program found on this PC, with the home folder shown as `~`. */
  found_path_display: string | null;
}

export interface StoreAddResult {
  ok: boolean;
  id: string;
  state: StoreState;
}

export interface AgentStoreApi {
  list: () => Promise<StoreList>;
  detail: (id: string) => Promise<StoreDetail>;
  add: (id: string, confirmedFound: boolean) => Promise<StoreAddResult>;
  remove: (id: string) => Promise<void>;
  recordInstall: (id: string, stepIndex: number) => Promise<void>;
  uninstallPlan: (id: string) => Promise<InstallPlan>;
  uninstallRun: (id: string, step: number, shownCommand: string) => Promise<InstallStarted>;
  recordCheck: (id: string, passed: number, total: number, version: string) => Promise<void>;
}

export const agentStoreApi: AgentStoreApi = {
  list: () => invoke<StoreList>('agent_store_list'),
  detail: (id) => invoke<StoreDetail>('agent_store_detail', { id }),
  add: (id, confirmedFound) => invoke<StoreAddResult>('agent_store_add', { id, confirmedFound }),
  remove: (id) => invoke<void>('agent_store_remove', { id }),
  recordInstall: (id, stepIndex) => invoke<void>('agent_store_record_install', { id, stepIndex }),
  uninstallPlan: (id) => invoke<InstallPlan>('agent_store_uninstall_plan', { id }),
  uninstallRun: (id, step, shownCommand) =>
    invoke<InstallStarted>('agent_store_uninstall_run', { id, step, shownCommand }),
  recordCheck: (id, passed, total, version) =>
    invoke<void>('agent_store_record_check', { id, passed, total, version }),
};
