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
    /** Answer `/questions` with these instead of the example question. */
    questionList?: Question[];
    digest?: boolean;
    /**
     * The Team group: absent or `false` = Team is off (the bridge answers `enabled: false`). An object
     * turns it on; `can` is what this phone may do (default: everything), `missing` the count of
     * updates the PC could not read.
     */
    team?: false | { can?: { dismiss: boolean; act: boolean }; missing?: number };
    /** Spoof navigator.userAgent (e.g. an iPhone, for the Add to Home Screen hint). */
    userAgent?: string;
    /** Bridge events delivered once the stream opens (`event: <name>`). */
    events?: { name: string; data: unknown }[];
    /**
     * The PC's attention snapshot `GET /attention` answers; absent = an older PC (404). With `items` the
     * counts are theirs (open blocking ones); without, `blocking` and `ready` are the counts and the list is empty.
     */
    attention?: {
      blocking: number;
      ready?: number;
      items?: import('../lib/attention').AttentionItem[];
      /** What this phone may do (default all). */
      device?: { can_answer: boolean; can_type: boolean; can_control: boolean };
      /**
       * What `POST /attention/{id}/answer` does: `answered` (default), `changed`, `gone`, `already_pc` (answered on
       * the PC first), `failed`, `hang` (never replies), `offline` (the request cannot leave the phone).
       */
      answer?: 'answered' | 'changed' | 'gone' | 'already_pc' | 'failed' | 'hang' | 'offline';
    };
    /** Extra launch query, as a notification tap would open it (`item=<id>`, `pane=<id>`). */
    search?: string;
  };
  /** Phone route: the abilities this phone was paired with (`GET /pair/me`); default: all four. */
  phoneCaps?: string[];
  /** Phone route: override the panes the mocked bridge lists. */
  phonePanes?: PaneInfo[];
  /** App theme the mocked settings store reports (default dark). */
  theme?: 'dark' | 'light';
  /** Cockpit: start as a brand-new install (no agent was ever opened here). Default: an install that has been used.
   *  A `firstRun` scenario is a brand-new install too. */
  newInstall?: boolean;
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
  /** Budget the mocked `budget_for_pane` returns per pane id (absent: no budget, the default). */
  budgets?: Record<string, import('../lib/budgets').BudgetView>;
  /** Pending `ask_user` questions the mocked `questions_list` returns. */
  questions?: Question[];
  /** Settings > Security: the two agent-authority switches the mocked backend reports. */
  authority?: { ask_before_agent_control: boolean; agents_may_merge: boolean };
  /** Settings > Security: lines of the audit log the mocked backend returns (newest first). */
  audit?: { seq: number; ts_ms: number; kind: string; who: string; summary: string }[];
  /** Team (Inbox > Team, the toast, Settings > Labs): what the mocked `team_state` returns. */
  team?: import('../lib/team').TeamState;
  /** Simple mode: turns the switch on and gives the panes, pages and answers it shows (see simple-fixtures.ts). */
  simple?: import('./simple-fixtures').SimpleScenario;
  /** First-run welcome: where it starts and what the mocked first task does. Absent = the welcome never shows. */
  firstRun?: import('./first-run-fixtures').FirstRunScenario;
  /** Worktrees that ship: what the mocked folder step, presets and GitHub commands answer. */
  ship?: import('./ship-fixtures').ShipScenario;
  /** Missions (Preview): the Labs switch and what the mocked `mission_*` commands answer. */
  missions?: import('./mission-fixtures').MissionScenario;
  /** Keep going: the picker rows, the hand-over lines per pane and the brief the mocked `keep_going_*` commands answer. */
  keepGoing?: import('./keep-going-fixtures').KeepGoingScenario;
  /** Branch from here: what the mocked `branch_lineage` and `branch_brief` commands answer (see branch-fixtures.ts). */
  branch?: import('./branch-fixtures').BranchScenario;
  /** Night Shift: what the state, plans and checks the mocked `night_*` commands answer (see night-fixtures.ts). */
  night?: import('./night-fixtures').NightScenario;
  /** Settings > Labs > Team: overrides on top of the example config (on a team as admin) the mocked `team_config_get` returns. */
  teamConfig?: Partial<import('../lib/team').TeamConfigView>;
  /** Set up agents > Install: what the mocked `agents_install_*` commands answer. */
  doctorInstall?: {
    /** The plan for the missing agent: winget (one step), prereq (Node.js first), script (vendor script), blocked (no winget). */
    plan: 'winget' | 'prereq' | 'script' | 'blocked';
    /** What the install window reports once Run was pressed. */
    after?: 'running' | 'done_ok' | 'cancelled' | 'restart';
  };
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
