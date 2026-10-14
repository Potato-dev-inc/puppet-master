// Scenario contract for the UI evidence harness (dev only).
import type { Account } from '../lib/accounts';
import type { Question } from '../lib/questions';
import type { PaneInfo } from '../lib/tauri-types';
import type { CanaryStatus } from '../lib/canary';

export interface HarnessScenario {
  /** `?harness=<name>`; unique across scenarios. */
  name: string;
  /** Which window to render. */
  route: 'cockpit' | 'settings' | 'phone' | 'askbar';
  /**
   * Window size in CSS px for the screenshot. `scripts/evidence/screenshot.mjs` reads this literal
   * from the scenario file; the default is 1440x900, or 390x844 for route `phone`.
   */
  window?: { width: number; height: number };
  /** Phone route: what the mocked PC bridge answers and how the page starts. */
  phone?: {
    /** Tab opened first: a pane id; default is the Agents list. */
    activeTab?: string;
    /** `false` hides the question card / the away digest. */
    questions?: boolean;
    digest?: boolean;
    /** Spoof navigator.userAgent (e.g. an iPhone, for the Add to Home Screen hint). */
    userAgent?: string;
    /** Bridge events delivered once the stream opens (`event: <name>`). */
    events?: { name: string; data: unknown }[];
  };
  /** Phone route: the abilities this phone was paired with (`GET /pair/me`); default: all four. */
  phoneCaps?: string[];
  /** Phone route: override the panes the mocked bridge lists. */
  phonePanes?: PaneInfo[];
  /** App theme the mocked settings store reports (default dark). */
  theme?: 'dark' | 'light';
  /** Cockpit: pane id preselected through `?select=`. */
  select?: string;
  /** Settings > Agents > Preload: the stored idle timeout in seconds (default 1200). */
  standbyIdleSecs?: number;
  /** Settings > Agents > Preload: the "held back" rows the status reports. */
  standbyBlocked?: { key: string; reason: string; fails: number; retry_in_ms: number; cold: boolean }[];
  /** Version checks (`canary_status`): what the mocked backend reports. Default: no agents flagged. */
  canary?: CanaryStatus;
  /** Settings: tab id passed as `?tab=`. */
  settingsTab?: string;
  /** User accounts the mocked backend reports (Settings > Agents > Accounts, the "+" menu). */
  accounts?: Account[];
  /** Cockpit: add an agent that has no pane of its own (only other agents can run it) to the "+" menu. */
  headlessOnlyAgent?: boolean;
  /** Cockpit: start with the "While you were away" digest showing (as after a return). */
  awayDigest?: boolean;
  /** Run info the mocked `pane_run_info` returns per pane id (default: only the example Claude pane has one). */
  runs?: Record<string, import('../lib/cockpit-api').PaneRunInfo>;
  /** Cockpit History: make `checkpoints_list` answer with this state instead of the example list. */
  checkpointsState?: 'too_big';
  /** Override the example panes. */
  panes?: PaneInfo[];
  /** Pending `ask_user` questions the mocked `questions_list` returns. */
  questions?: Question[];
  /** Settings > Security: the two agent-authority switches the mocked backend reports. */
  authority?: { ask_before_agent_control: boolean; agents_may_merge: boolean };
  /** Settings > Security: lines of the audit log the mocked backend returns (newest first). */
  audit?: { seq: number; ts_ms: number; kind: string; who: string; summary: string }[];
  /** Team (Inbox > Team, the toast, Settings > Labs): what the mocked `team_state` returns. */
  team?: import('../lib/team').TeamState;
  /** Worktrees that ship: what the mocked folder step, presets and GitHub commands answer. */
  ship?: import('./ship-fixtures').ShipScenario;
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
