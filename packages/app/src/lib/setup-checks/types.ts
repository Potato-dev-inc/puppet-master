// Shared shapes for "Check my setup". A check turns one status read into rows; a row is one plain sentence and at most
// one fix button. Checks never run a command themselves: they only call the `SetupCheckDeps` they are given.
import type { AddonList } from '../acpAddon';
import type { AgentHealth } from '../agent-health';
import type { SandboxState } from '../potato-sandbox';

export type CheckStatus = 'ok' | 'warn' | 'fail';

/** What the one fix button does. The page maps each kind to an action (see `runFix`). */
export type FixKind = 'signin' | 'installAddon' | 'addKey' | 'openSandbox';

export interface FixAction {
  kind: FixKind;
  label: string;
  /** Agent id for `signin` and `installAddon`. */
  agentId?: string;
}

export interface CheckRow {
  /** Stable id, used as the React key and in tests. */
  id: string;
  status: CheckStatus;
  /** One plain sentence. Shown as is, and copied as is into the report. */
  sentence: string;
  fix: FixAction | null;
}

/** The status reads the checks need. Defaults are the Tauri commands (see `index.ts`); tests pass fakes. */
export interface SetupCheckDeps {
  agentHealth: () => Promise<AgentHealth[]>;
  /** Only whether a DeepSeek key is saved. The value is never read. */
  deepseekKeySaved: () => Promise<boolean>;
  sandboxState: () => Promise<SandboxState>;
  addons: () => Promise<AddonList>;
}

/** Facts the page already has (from Settings), passed in so checks stay pure. */
export interface SetupCheckContext {
  bridgeUrl: string | null;
  /** Labs switches as saved in settings. */
  labs: Readonly<Record<string, boolean | undefined>>;
}
