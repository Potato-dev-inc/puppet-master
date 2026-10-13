// Scenario contract for the UI evidence harness (dev only).
import type { Account } from '../lib/accounts';
import type { Question } from '../lib/questions';
import type { PaneInfo } from '../lib/tauri-types';

export interface HarnessScenario {
  /** `?harness=<name>`; unique across scenarios. */
  name: string;
  /** Which window to render. */
  route: 'cockpit' | 'settings';
  /** App theme the mocked settings store reports (default dark). */
  theme?: 'dark' | 'light';
  /** Cockpit: pane id preselected through `?select=`. */
  select?: string;
  /** Settings: tab id passed as `?tab=`. */
  settingsTab?: string;
  /** User accounts the mocked backend reports (Settings > Agents > Accounts, the "+" menu). */
  accounts?: Account[];
  /** Cockpit: start with the "While you were away" digest showing (as after a return). */
  awayDigest?: boolean;
  /** Override the example panes. */
  panes?: PaneInfo[];
  /** Pending `ask_user` questions the mocked `questions_list` returns. */
  questions?: Question[];
  /** Runs once after the first paint (and after the panes loaded); click things, press keys. */
  after?: (api: HarnessDom) => void | Promise<void>;
}

export interface HarnessDom {
  sleep: (ms: number) => Promise<void>;
  /** Wait until a selector matches (default 4s). */
  waitFor: (selector: string, timeoutMs?: number) => Promise<Element | null>;
  click: (selector: string) => Promise<boolean>;
  /** Click the first element matching `selector` whose text contains `text`. */
  clickText: (selector: string, text: string) => Promise<boolean>;
  key: (key: string, mods?: { ctrl?: boolean; shift?: boolean }) => void;
}
