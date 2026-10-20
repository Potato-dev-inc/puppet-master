// Agent Wizard front end (docs/specs/agent-wizard/SPEC.md): the typed API over the Tauri commands, the
// drafters, and the draft loop. Rust is deterministic and does no AI work; the loop that asks an AI for
// a definition lives here. It runs: build the prompt, ask the drafter, take exactly one ```toml block,
// verify, feed the checker's text back verbatim, then (once the person agrees) the live test, at most
// five rounds in all.
import { invoke } from '@tauri-apps/api/core';
import {
  buildPrompt,
  LOCAL_PROMPT_BUDGET,
  maskEmails,
  PROMPT_VERSION,
  REMOTE_PROMPT_BUDGET,
  type AnsweredQuestion,
  type CaptureFacts,
  type ProbeRun,
  type PromptFacts,
  type PromptOptions,
  type Sample,
} from './agentWizardPrompt';
import { streamLlm } from './llm';
import { resolveApiKey } from './secrets';
import { listModels, loadSettings } from './settings';

export { PROMPT_VERSION };

export const MAX_ROUNDS = 5;

// ---- wire types (SPEC section 4) ------------------------------------------------------------

export interface StartInfo {
  session: string;
  state: string;
  command: string;
  id_suggestion: string;
  program: { path: string; display: string; kind: 'native' | 'shim' };
  version: { ok: boolean; args: string[]; text: string; number: string | null };
  native_candidates: string[];
  cost_note: string;
  limits: Record<string, number>;
}

export interface ProbeInfo {
  state: string;
  runs: Array<ProbeRun & { args: string[]; truncated: boolean; bytes: number }>;
  subcommands: string[];
  skipped: Array<{ args: string[]; reason: string }>;
}

export interface CheckError {
  stage: string;
  code: string;
  path: string;
  line: number | null;
  column: number | null;
  message: string;
  fix: string;
  excerpt?: string;
}

/** The commands the live test will run, rendered by Rust (the test message is `<test message>`). */
export interface PlannedRun {
  program: { path: string; display: string };
  stages: Array<{ name: string; label: string; args: string[] }>;
  /** The task goes to the program's standard input. */
  stdin: string | null;
  env: Array<{ name: string; value: string }>;
  env_unset: string[];
  /** Sent back with the live-test request; Rust refuses to run unless it matches. */
  fingerprint: string;
}

/** The pane the visible test would start: what the person confirms (`fingerprint` is sent back). */
export interface PlannedVisible {
  program: { path: string; display: string };
  args: string[];
  env: Array<{ name: string; value: string }>;
  env_unset: string[];
  cwd: string;
  grid: { cols: number; rows: number };
  fingerprint: string;
}

/** What the guided capture would start, shown before the person confirms. */
export interface CapturePlan {
  program: { path: string; display: string };
  args: string[];
  cwd: string;
  grid: { cols: number; rows: number };
  fingerprint: string;
}

export type CaptureStepId = 'start' | 'ready' | 'message' | 'work' | 'permission' | 'exit';
export type CaptureStepStatus = 'pending' | 'running' | 'needs_you' | 'done' | 'skipped' | 'stuck' | 'failed';
export type CaptureAction = 'skip' | 'retry' | 'stuck' | 'ready' | 'try_exit' | 'closed' | 'done' | 'question';

/** One look the capture kept, masked: what the classifier said and what the wizard made of it. */
export interface CaptureDebugFrame {
  ms: number;
  step: CaptureStepId;
  step_status: CaptureStepStatus;
  classifier: string;
  reading: string;
  stable_ms: number;
  quiet_ms: number;
  needs_you: string | null;
  screen: string;
}

/** The last looks of a capture, for "why did it stop" (`capture` op `debug`). */
export interface CaptureDebug {
  at_ms: number;
  frames: CaptureDebugFrame[];
}

export interface CaptureStep {
  id: CaptureStepId;
  label: string;
  status: CaptureStepStatus;
  note: string;
  frames: number;
}

/** The drafter AI's one-word look at a screen the wizard could not read ("Finish without me", SPEC 11.19). */
export type Verdict =
  | 'ready_for_input'
  | 'working'
  | 'question_trust_or_once'
  | 'question_other'
  | 'sign_in_or_payment'
  | 'error'
  | 'unknown';

/** What Rust did about a verdict. `stop` is set when the run must stop (sign-in or payment). */
export interface VerdictReply {
  decision: 'pressed_ready' | 'safe_answerer' | 'wait' | 'wait_then_skip' | 'stop' | 'marked_stuck' | 'stopped_test' | 'refused';
  verdict: Verdict;
  why?: string;
  stop?: { code: string; message: string };
  status?: CaptureStatus;
}

/**
 * What Rust made of the AI look's choice for a menu the safe answerer could not read (SPEC 11.20). `accepted` is true when
 * Rust checked the option and the keys against the screen and the safety rules and the answerer will press them, false with
 * `why` when it refused; for the visible test it is null (the test answers with its next look: `VisibleTestStatus.menu_choice`).
 */
export interface MenuChoiceReply {
  accepted: boolean | null;
  label?: string;
  why?: string;
  status?: CaptureStatus;
}

/** What an autonomous run tells `finish`: the decisions it took (for the record) and that it wants the approval. */
export interface AutoFinish {
  decisions: string[];
  approve: boolean;
}

export interface CaptureStatus {
  state: 'none' | 'running' | 'finished';
  /** True once a finished capture is stored in the session. */
  captured: boolean;
  pane?: string;
  grid?: { cols: number; rows: number };
  alive?: boolean;
  step?: CaptureStepId;
  steps?: CaptureStep[];
  screen_kind?: string;
  /** The screen reads as working by its words but has not moved for seconds: it is read as unknown. */
  frozen?: boolean;
  stable_ms?: number;
  settled?: boolean;
  /** A short name of the picture on the screen (the AI look is asked once per picture). */
  fingerprint?: string;
  /** Set when the run was stopped (a sign-in or payment screen); nothing is typed until a retry. */
  halted?: string | null;
  /** `unreadable`: the safe answerer could not find the menu or its highlighted row (an AI look may name the option). */
  needs_you?: { step: CaptureStepId; kind: string; message: string; unreadable?: boolean } | null;
  /** The identity of the question the safe answerer is looking at (the AI look's answer is bound to it). */
  question_fp?: string | null;
  /** How the answerer's parser read the menu of the question it is looking at (every option with its class), masked. */
  menu_read?: string | null;
  /** Rows drawn highlighted (inverse video, a band, bold), masked: a hint for the AI look when the text shows no cursor. */
  menu_look?: { emphasis: string[] };
  /** Setup questions the wizard answered for the person so far (labels only). `via` is `ai_look` when an AI look named it. */
  answered?: Array<{ place: string; flavour: string; label: string; keys: string; via?: string; menu?: string }>;
  auto_answer?: boolean;
  /** The pane's screen, masked, for the preview in the wizard window. */
  screen?: string;
  can?: CaptureAction[];
}

export interface VisibleCheck {
  name: string;
  label: string;
  status: 'passed' | 'failed' | 'skipped';
  note: string;
  ms: number;
}

export interface VisibleTestResult {
  ok: boolean;
  state: string;
  checks: VisibleCheck[];
  errors: CheckError[];
  /** The errors written for an AI (the screen at the failing check included). Goes back into the next prompt. */
  text: string;
  version: string | null;
  passed: number;
  total: number;
  agent_error?: { check: string; message: string; external: boolean } | null;
  /** Setup questions the wizard answered for the person in the test pane. */
  answered?: AnsweredQuestion[];
}

/** What the visible test is doing right now (read while the test call is still running). */
export interface VisibleTestStatus {
  running: boolean;
  check?: string | null;
  /** `kind` is the question's flavour (trust, login, question...) or `stuck` after a long wait. */
  needs_you?: { message: string; kind?: string; unreadable?: boolean; emphasis?: string[] } | null;
  checks?: VisibleCheck[];
  screen?: string;
  answered?: AnsweredQuestion[];
  /** What the test made of the AI look's choice (`accepted: false` with `why`), once it looked at it. */
  menu_choice?: { accepted: boolean; label?: string; why?: string } | null;
  /** How the answerer read the menu of the question in the test pane (every option with its class), masked. */
  menu_read?: string | null;
}

export interface VerifyResult {
  ok: boolean;
  state: string;
  errors: CheckError[];
  /** The errors written for an AI. Goes back into the next prompt verbatim. */
  text: string;
  warnings: string[];
  rendered?: { headless_argv: string[] | null; resume_argv: string[] | null; tui_argv?: string[] };
  checked: { fixtures: number; rules: number };
  samples_replayed: number;
  sha256?: string;
  /**
   * What the live test would run for this draft. Present when `ok`, and also when the draft only fails
   * the shared rules or the fixture checks (`can_livetest`); absent for a draft that is not valid.
   */
  planned?: PlannedRun;
  can_livetest?: boolean;
  /** What the visible test would start (a draft with `[tui]` after a capture); absent for a draft that is not valid. */
  planned_visible?: PlannedVisible;
  can_visibletest?: boolean;
}

export interface StageReport {
  name: string;
  label: string;
  status: 'passed' | 'failed' | 'skipped';
  args: string[];
  exit: number | null;
  duration_ms: number;
  timed_out: boolean;
  result: string | null;
  session: string | null;
  error_code: string | null;
  /** The program was still running after its done event and was stopped, so `exit` is not its own. */
  stopped_early?: boolean;
  problems: string[];
  note: string | null;
  stdout: string;
  stderr: string;
}

export interface LiveResult {
  ok: boolean;
  state: string;
  stages: StageReport[];
  errors: CheckError[];
  text: string;
  tested_version: string | null;
  fixtures: string[];
  /**
   * The program's own error behind the failure (`external`: sign-in, quota, provider or network, which
   * no new draft can fix). Null when the failure is the profile's.
   */
  agent_error?: AgentError | null;
}

export interface AgentError {
  stage: string;
  message: string;
  external: boolean;
}

/** Logo, rule, box-drawing, block and braille characters: the strokes of a banner, never words. */
const FRAME_CHAR = /[─-▟■-◿⠀-⣿|+=_~#-]/;
/** A lone `<Name> CLI 1.4.2` or `v1.4.2` banner row, a logo in front of it or not. */
const BANNER_ROW = /^(?:[a-z][\w.+-]*\s+){0,4}(?:cli\s+|code\s+|agent\s+|tui\s+)?v?\d+\.\d+(?:\.\d+)?(?:[-+][\w.]+)?\s*(?:\([^)]*\))?\.?$/i;
const ERRORISH = /\b(error|errors|failed|failure|exception|traceback|panicked|fatal|cannot|can't|could not|couldn't|unable to|not found|denied|timed out|timeout)\b/i;

/**
 * Whether `message` is text an error can be shown with: it holds words and is not a logo, a rule or a
 * `<Name> CLI 1.4.2` banner (the first line an inline TUI prints). The program's own error is only a stop
 * when there is such text (SPEC 4.4): without it the failure is the draft's and the next round fixes it.
 */
export function isRealErrorMessage(message: string | null | undefined): boolean {
  const first = (message ?? '').split('\n').map((l) => l.trim()).find((l) => l.length > 0) ?? '';
  if (!first) return false;
  const cells = [...first].filter((c) => !/\s/.test(c));
  if (cells.filter((c) => FRAME_CHAR.test(c)).length * 2 >= cells.length) return false;
  if ([...first].filter((c) => /\p{L}/u.test(c)).length < 3) return false;
  const core = first.replace(/^[\s─-▟■-◿⠀-⣿|+=_~#-]+|[\s─-▟■-◿⠀-⣿|+=_~#-]+$/g, '');
  return !(BANNER_ROW.test(core) && !ERRORISH.test(core));
}

/** The program's error, only when it carries real error text. */
function realAgentError(error: AgentError | null | undefined): AgentError | null {
  return error && isRealErrorMessage(error.message) ? error : null;
}

export interface Provenance {
  drafter: string;
  drafter_kind: 'local_agent' | 'openrouter' | 'other';
  model: string | null;
  prompt_version: string;
  rounds: number;
}

/** The wizard's own approval after a "Finish without me" run (SPEC 11.19). */
export interface ApprovalInfo {
  approved: boolean;
  approved_by?: string;
  /** How many checks had passed. */
  count?: number;
  checks?: string[];
  reason?: string;
}

export interface FinishResult {
  ok: boolean;
  id: string;
  file: string;
  /** Present for a session started with "Finish without me". */
  approval?: ApprovalInfo;
  fixtures_dir: string;
  provenance_file: string;
  state: string;
  fixtures: { passed: number; total: number };
  /** The definitions listing after the registry reload (so the caller need not list again). */
  listing?: unknown;
}

export interface LocalDrafter {
  id: string;
  label: string;
}

/** What the wizard UI and the loop talk to; tests pass a mock. */
export interface WizardApi {
  /** `autoAnswer`: the person left "Answer setup questions for me" on (SPEC 11.16). `autonomous`: "Finish without me" (11.19). */
  start(command: string, autoAnswer?: boolean, autonomous?: boolean): Promise<StartInfo>;
  probe(session: string): Promise<ProbeInfo>;
  /** `expect` holds the drafter's expectation blocks for the screen captures, by capture name. */
  verify(session: string, toml: string, expect?: Record<string, string>): Promise<VerifyResult>;
  /** Runs the live test. `plan` is the fingerprint of the commands the person confirmed. */
  livetest(session: string, plan: string): Promise<LiveResult>;
  /** Runs the visible test. `plan` is the fingerprint of `planned_visible` the person confirmed. */
  visibletest(session: string, plan: string): Promise<VisibleTestResult>;
  /** What the visible test is doing now (poll while `visibletest` runs). */
  testStatus(session: string): Promise<VisibleTestStatus>;
  /** Stop the visible test while a question is up (`stop`), or skip the permission check (`skip`). */
  testAct?(session: string, action: 'skip' | 'stop', reason?: string): Promise<void>;
  /** "Finish without me": the AI look at the capture pane. Rust decides what the verdict may cause. */
  verdict?(session: string, verdict: Verdict): Promise<VerdictReply>;
  /** The same for the visible test's pane. */
  testVerdict?(session: string, verdict: Verdict): Promise<VerdictReply>;
  /** "Finish without me": the AI look named an option and keys for a menu Rust could not read; Rust checks them. */
  menuChoice?(session: string, option: string, keys: string[], fingerprint?: string): Promise<MenuChoiceReply>;
  /** The same for the visible test's pane. */
  testMenuChoice?(session: string, option: string, keys: string[]): Promise<MenuChoiceReply>;
  /** Open the visible test's pane (the question on it is the person's). */
  testShow?(session: string): Promise<void>;
  capturePlan(session: string, args: string[]): Promise<CapturePlan>;
  /** Starts the guided capture; `plan` is the fingerprint the person confirmed. */
  captureStart(session: string, args: string[], plan: string): Promise<CaptureStatus>;
  captureStatus(session: string): Promise<CaptureStatus>;
  captureAct(session: string, action: CaptureAction, step?: CaptureStepId): Promise<CaptureStatus>;
  /** The last looks the capture kept (masked), while it runs and after it stopped. */
  captureDebug?(session: string): Promise<CaptureDebug | null>;
  /** Writes the debug file of a run that stopped (the last looks, the menu as read, the decisions, why) and says where. */
  captureDump?(session: string, reason: string): Promise<{ path: string; copy?: string | null } | null>;
  /** Opens the capture pane as a window (the person answers its questions there). */
  captureShow(session: string): Promise<void>;
  /** Stops the capture and returns the screens as the drafter will see them (test words masked). */
  captureFinish(session: string): Promise<CaptureFacts | null>;
  captureDiscard(session: string): Promise<void>;
  finish(session: string, provenance: Provenance, auto?: AutoFinish): Promise<FinishResult>;
  cancel(session: string): Promise<unknown>;
  /** One read-only question to an installed local agent, in the session's work folder. */
  ask(session: string, agent: string, prompt: string): Promise<string>;
  /** Installed local agents that can answer a read-only question. */
  drafters(): Promise<LocalDrafter[]>;
}

export const tauriWizardApi: WizardApi = {
  start: (command, autoAnswer, autonomous) =>
    invoke<StartInfo>('agent_wizard_start', { command, autoAnswer: autoAnswer ?? false, autonomous: autonomous ?? false }),
  probe: (session) => invoke<ProbeInfo>('agent_wizard_probe', { session }),
  verify: (session, toml, expect) => invoke<VerifyResult>('agent_wizard_verify', { session, toml, expect: expect ?? null }),
  livetest: (session, plan) => invoke<LiveResult>('agent_wizard_livetest', { session, confirm: true, plan }),
  visibletest: (session, plan) => invoke<VisibleTestResult>('agent_wizard_visibletest', { session, confirm: true, plan }),
  testStatus: (session) => invoke<VisibleTestStatus>('agent_wizard_capture', { body: { session, op: 'test_status' } }),
  testAct: async (session, action, reason) => {
    await invoke('agent_wizard_capture', { body: { session, op: 'test_act', action, ...(reason ? { reason } : {}) } });
  },
  verdict: (session, verdict) => invoke<VerdictReply>('agent_wizard_capture', { body: { session, op: 'verdict', verdict } }),
  testVerdict: (session, verdict) => invoke<VerdictReply>('agent_wizard_capture', { body: { session, op: 'test_verdict', verdict } }),
  menuChoice: (session, option, keys, fingerprint) =>
    invoke<MenuChoiceReply>('agent_wizard_capture', { body: { session, op: 'menu_choice', option, keys, ...(fingerprint ? { fingerprint } : {}) } }),
  testMenuChoice: (session, option, keys) => invoke<MenuChoiceReply>('agent_wizard_capture', { body: { session, op: 'test_menu_choice', option, keys } }),
  testShow: async (session) => {
    await invoke('agent_wizard_capture', { body: { session, op: 'show' } });
  },
  capturePlan: async (session, args) => (await invoke<{ plan: CapturePlan }>('agent_wizard_capture', { body: { session, op: 'plan', args } })).plan,
  captureStart: (session, args, plan) =>
    invoke<CaptureStatus>('agent_wizard_capture', { body: { session, op: 'start', args, confirm: true, plan } }),
  captureStatus: (session) => invoke<CaptureStatus>('agent_wizard_capture', { body: { session, op: 'status' } }),
  captureAct: (session, action, step) => invoke<CaptureStatus>('agent_wizard_capture', { body: { session, op: 'act', action, step } }),
  captureDebug: async (session) => (await invoke<{ debug: CaptureDebug | null }>('agent_wizard_capture', { body: { session, op: 'debug' } })).debug,
  captureDump: (session, reason) => invoke<{ path: string; copy?: string | null }>('agent_wizard_capture', { body: { session, op: 'dump', reason } }),
  captureShow: async (session) => {
    await invoke('agent_wizard_capture', { body: { session, op: 'show' } });
  },
  captureFinish: async (session) =>
    (await invoke<{ captures: CaptureFacts | null }>('agent_wizard_capture', { body: { session, op: 'finish' } })).captures,
  captureDiscard: async (session) => {
    await invoke('agent_wizard_capture', { body: { session, op: 'discard' } });
  },
  finish: (session, provenance, auto) => invoke<FinishResult>('agent_wizard_finish', { session, provenance, autonomous: auto ?? null }),
  cancel: (session) => invoke('agent_wizard_cancel', { session }),
  ask: async (session, agent, prompt) =>
    (await invoke<{ answer: string }>('agent_wizard_ask', { session, agent, prompt: maskEmails(prompt) })).answer,
  drafters: async () => (await invoke<Array<LocalDrafter & { ready?: boolean }>>('askbar_agents')).map(({ id, label }) => ({ id, label })),
};

/** The error text of a failed command: `CODE: message`. */
export function splitError(e: unknown): { code: string; message: string } {
  const text = e instanceof Error ? e.message : typeof e === 'string' ? e : String(e);
  const m = /^([A-Z][A-Z0-9_]{2,}): ([\s\S]*)$/.exec(text);
  return m ? { code: m[1], message: m[2] } : { code: '', message: text };
}

// ---- drafters --------------------------------------------------------------------------------

export type Drafter =
  | { kind: 'local_agent'; id: string; label: string }
  | { kind: 'openrouter'; model: string; label: string };

/** Claude Code first, then Codex, then whatever else can answer a read-only question. */
export function chooseDefaultDrafter(list: LocalDrafter[]): LocalDrafter | null {
  for (const id of ['claude', 'codex']) {
    const hit = list.find((d) => d.id === id);
    if (hit) return hit;
  }
  return list[0] ?? null;
}

export function orderDrafters(list: LocalDrafter[]): LocalDrafter[] {
  const rank = (id: string) => (id === 'claude' ? 0 : id === 'codex' ? 1 : 2);
  return [...list].sort((a, b) => rank(a.id) - rank(b.id));
}

export function promptBudget(drafter: Drafter): number {
  return drafter.kind === 'openrouter' ? REMOTE_PROMPT_BUDGET : LOCAL_PROMPT_BUDGET;
}

export function provenanceOf(drafter: Drafter, rounds: number): Provenance {
  return {
    drafter: drafter.kind === 'openrouter' ? `OpenRouter ${drafter.model}` : `${drafter.label} (local)`,
    drafter_kind: drafter.kind,
    model: drafter.kind === 'openrouter' ? drafter.model : null,
    prompt_version: PROMPT_VERSION,
    rounds,
  };
}

const OPENROUTER_SYSTEM = 'You write agent definition files for Puppet Master. Follow the instructions in the user message exactly.';

type StreamFn = typeof streamLlm;

/** One OpenRouter chat call through the app's own client; the key is read at call time and never stored here. */
export async function askOpenRouter(opts: {
  apiKey: string;
  model: string;
  prompt: string;
  signal?: AbortSignal;
  stream?: StreamFn;
}): Promise<string> {
  const stream = opts.stream ?? streamLlm;
  const res = await stream(
    'openrouter',
    opts.apiKey,
    { model: opts.model, system: OPENROUTER_SYSTEM, messages: [{ role: 'user', content: opts.prompt }], tools: [], max_tokens: 8000 },
    { onTextDelta: () => undefined, signal: opts.signal },
  );
  const text = res.content
    .filter((b): b is { type: 'text'; text: string } => b.type === 'text')
    .map((b) => b.text)
    .join('\n')
    .trim();
  if (!text) throw new Error('OpenRouter returned an empty answer.');
  return text;
}

export interface OpenRouterSupport {
  /** Models to offer. Empty when none are set up. */
  models(): Promise<Array<{ id: string; label: string }>>;
  /** True when a key is saved (Settings > Keys). */
  hasKey(): Promise<boolean>;
  ask(model: string, prompt: string, signal?: AbortSignal): Promise<string>;
}

export const openRouterSupport: OpenRouterSupport = {
  models: async () => {
    const settings = await loadSettings();
    return listModels(settings)
      .filter((m) => m.provider === 'openrouter')
      .map((m) => ({ id: m.model_id, label: m.label ?? m.model_id }));
  },
  hasKey: async () => Boolean(await resolveApiKey(await loadSettings(), 'openrouter')),
  ask: async (model, prompt, signal) => {
    const key = await resolveApiKey(await loadSettings(), 'openrouter');
    if (!key) throw new Error('Add your OpenRouter key in Settings > Keys first.');
    return askOpenRouter({ apiKey: key, model, prompt: maskEmails(prompt), signal });
  },
};

// ---- reading the drafter's reply -------------------------------------------------------------

export type Extracted = { ok: true; toml: string } | { ok: false; problem: string };

/**
 * Exactly one fenced ```toml block. Zero or several is a problem the drafter is told about. A single
 * unlabelled fence is accepted when it starts like a definition (`schema_version`).
 */
export function extractTomlBlock(reply: string): Extracted {
  const fence = '`'.repeat(3);
  const labelled = [...reply.matchAll(/```[ \t]*toml[ \t]*\r?\n([\s\S]*?)```/gi)].map((m) => m[1].trim());
  if (labelled.length === 1) return nonEmpty(labelled[0]);
  if (labelled.length > 1) {
    return {
      ok: false,
      problem: `Your reply had ${labelled.length} ${fence}toml blocks. Reply with exactly one ${fence}toml block that holds the whole file.`,
    };
  }
  const bare = [...reply.matchAll(/```[ \t]*\r?\n([\s\S]*?)```/g)].map((m) => m[1].trim());
  if (bare.length === 1 && /^\s*(#.*\n\s*)*schema_version\s*=/.test(bare[0])) return nonEmpty(bare[0]);
  return {
    ok: false,
    problem: `Your reply had no ${fence}toml block. Reply with exactly one ${fence}toml block that holds the whole file.`,
  };
}

function nonEmpty(toml: string): Extracted {
  return toml ? { ok: true, toml } : { ok: false, problem: 'The toml block was empty. Reply with the whole file.' };
}

export type ExtractedDraft = { ok: true; toml: string; expects: Record<string, string> } | { ok: false; problem: string };

const EXPECT_HEAD = /^\s*#\s*expect:\s*([A-Za-z0-9_]+)\s*\r?\n/;

/**
 * The visible path's reply: exactly one definition block, plus any number of blocks whose first line is
 * `# expect: <capture name>` (the extra expectations for that capture). An expectation block with no
 * definition, or two blocks for the same capture, is a problem the drafter is told about.
 */
export function extractDraft(reply: string): ExtractedDraft {
  const fence = '`'.repeat(3);
  const blocks = [...reply.matchAll(/```[ \t]*toml[ \t]*\r?\n([\s\S]*?)```/gi)].map((m) => m[1]);
  const expects: Record<string, string> = {};
  const definitions: string[] = [];
  for (const block of blocks) {
    const head = EXPECT_HEAD.exec(block);
    if (!head) {
      definitions.push(block.trim());
      continue;
    }
    const name = head[1];
    if (name in expects) {
      return { ok: false, problem: `Your reply had two expectation blocks for ${name}. Give each capture at most one block.` };
    }
    expects[name] = block.slice(head[0].length).trim();
  }
  if (definitions.length === 0 && blocks.length === 0) {
    const single = extractTomlBlock(reply);
    return single.ok ? { ok: true, toml: single.toml, expects } : single;
  }
  if (definitions.length === 0) {
    return { ok: false, problem: `Your reply had only expectation blocks. The first ${fence}toml block must hold the whole file.` };
  }
  if (definitions.length > 1) {
    return {
      ok: false,
      problem: `Your reply had ${definitions.length} ${fence}toml blocks that are not expectation blocks. Reply with exactly one ${fence}toml block that holds the whole file; expectation blocks start with a "# expect: <capture>" line.`,
    };
  }
  const only = nonEmpty(definitions[0]);
  return only.ok ? { ok: true, toml: only.toml, expects } : only;
}

/** Does the help text mention a one-task mode (non-interactive, print, exec, JSON output)? Then it is offered as an extra. */
export function offersOneTaskMode(probe: ProbeInfo): boolean {
  const text = probe.runs.map((r) => r.text).join('\n');
  return /non-?interactive|headless|--print\b|\bexec\b|--prompt\b|--message\b|one[- ]?shot|--output-format|--format\b.{0,24}json|--json\b|stream-json|json[- ]?lines/i.test(text);
}

// ---- facts for the prompt --------------------------------------------------------------------

export function factsFrom(start: StartInfo, probe: ProbeInfo, captures?: CaptureFacts | null): PromptFacts {
  return {
    command: start.command,
    idSuggestion: start.id_suggestion,
    version: start.version.number ?? (start.version.text || null),
    programKind: start.program.kind,
    nativeCandidates: start.native_candidates,
    probes: probe.runs.map((r) => ({
      label: r.label,
      text: r.text,
      exit: r.exit,
      timed_out: r.timed_out,
      duplicate_of: r.duplicate_of ?? null,
    })),
    captures: captures ?? null,
  };
}

/** The real outputs of a live test, as the prompt shows them to the drafter. */
export function samplesFrom(live: LiveResult): Sample[] {
  return live.stages
    .filter((s) => s.status !== 'skipped' && (s.stdout || s.stderr || s.args.length))
    .map((s) => ({
      stage: s.name,
      args: s.args,
      exit: s.exit,
      stdout: s.stdout,
      stderr: s.stderr,
      result: s.result,
      session: s.session,
      error_code: s.error_code,
    }));
}

// ---- the draft loop --------------------------------------------------------------------------

export type StepId = 'help' | 'capture' | 'draft' | 'check' | 'fix' | 'visible' | 'live';

export interface ProgressEvent {
  step: StepId;
  status: 'running' | 'done' | 'failed';
  round: number;
  detail?: string;
}

export type LoopResult =
  | {
      status: 'tested';
      toml: string;
      expects: Record<string, string>;
      rounds: number;
      verify: VerifyResult;
      /** The one-task mode's live test, when the draft has a `[headless]` part. */
      live: LiveResult | null;
      /** The visible test, when the session captured the screens. */
      visible: VisibleTestResult | null;
    }
  | { status: 'declined'; toml: string; expects: Record<string, string>; rounds: number; verify: VerifyResult }
  | {
      status: 'agent_error';
      toml: string;
      expects: Record<string, string>;
      rounds: number;
      verify: VerifyResult;
      live: LiveResult | null;
      visible: VisibleTestResult | null;
      error: AgentError;
    }
  | { status: 'gave_up'; rounds: number; lastToml: string; lastText: string }
  /** "Finish without me": a screen asked for a sign-in or a payment step. Nothing more is typed. */
  | { status: 'sign_in'; rounds: number; message: string }
  | { status: 'cancelled'; rounds: number };

/** Runs next to the visible test (which blocks until it is done) and handles its "needs you" moments. */
export interface TestWatcher {
  /** The test call returned: stop looking. */
  stop(): void;
  /** The plain message when a screen asked for a sign-in or a payment step, else null. */
  signIn(): string | null;
}

/** "Finish without me" for the draft loop (SPEC 11.19). */
export interface AutonomousLoop {
  /** One plain, short sentence per automatic decision ("Will run: nimbus --x"). */
  onDecision(text: string): void;
  /** Starts the watcher for a visible test that is about to run. */
  watchTest?(session: string): TestWatcher;
}

export interface LoopOptions {
  /** `visibletest` is only needed for a session that captured screens. */
  api: Pick<WizardApi, 'verify' | 'livetest'> & Partial<Pick<WizardApi, 'visibletest'>>;
  session: string;
  facts: PromptFacts;
  /** Sends one prompt to the drafter and returns its reply. */
  ask: (prompt: string, round: number) => Promise<string>;
  maxChars: number;
  maxRounds?: number;
  /** More prompt options: the benchmark's leave-one-out, or "add the one-task mode to this file". */
  promptOptions?: Partial<Omit<PromptOptions, 'maxChars' | 'feedback'>>;
  /**
   * Called before a live test whose planned commands differ from the ones already confirmed (so: the
   * first time, and again when a fix changes the arguments). `false` stops the loop with `declined`.
   */
  confirmLive?: (planned: PlannedRun) => Promise<boolean>;
  /** The same for the visible test (the pane it would start). Without it the visible test is not run. */
  confirmVisible?: (planned: PlannedVisible) => Promise<boolean>;
  /**
   * "Finish without me": nobody confirms the planned commands (they are logged through `onDecision` and kept in
   * `<id>.wizard.json`; Rust still checks the fingerprint and refuses bypass-style flags), a draft only counts as
   * tested when the checker had no problem with it at all, and the visible test is watched for "needs you" moments.
   */
  autonomous?: AutonomousLoop;
  /** The pane plan the person already confirmed in an earlier loop (so the same pane is not asked about again). */
  confirmedVisible?: string | null;
  onEvent?: (event: ProgressEvent) => void;
  signal?: AbortSignal;
}

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? '' : 's'}`;

export async function runDraftLoop(opts: LoopOptions): Promise<LoopResult> {
  const max = opts.maxRounds ?? MAX_ROUNDS;
  const emit = (e: ProgressEvent) => opts.onEvent?.(e);
  let feedback: PromptOptions['feedback'];
  let samples: Sample[] = [];
  let lastToml = opts.promptOptions?.addOneTaskMode?.toml ?? '';
  let lastExpects: Record<string, string> = opts.promptOptions?.addOneTaskMode?.expects ?? {};
  let lastText = '';
  let confirmedPlan: string | null = null;
  let confirmedVisible: string | null = opts.confirmedVisible ?? null;
  let lastAgentError: string | null = null;
  let round = 0;
  const stopped = () => opts.signal?.aborted === true;
  const visibleMode = Boolean(opts.facts.captures);

  while (round < max) {
    round += 1;
    if (stopped()) return { status: 'cancelled', rounds: round - 1 };
    const writing: StepId = round === 1 ? 'draft' : 'fix';
    emit({ step: writing, status: 'running', round });
    const prompt = buildPrompt({ ...opts.facts, samples }, { ...opts.promptOptions, maxChars: opts.maxChars, feedback });
    let reply: string;
    try {
      reply = await opts.ask(prompt.text, round);
    } catch (e) {
      if (stopped()) return { status: 'cancelled', rounds: round };
      emit({ step: writing, status: 'failed', round, detail: splitError(e).message });
      throw e;
    }
    if (stopped()) return { status: 'cancelled', rounds: round };
    emit({ step: writing, status: 'done', round });

    const extracted = extractDraft(reply);
    if (!extracted.ok) {
      emit({ step: 'check', status: 'failed', round, detail: extracted.problem });
      lastText = extracted.problem;
      feedback = { previousToml: lastToml, previousExpects: lastExpects, text: extracted.problem };
      continue;
    }
    lastToml = extracted.toml;
    lastExpects = extracted.expects;

    emit({ step: 'check', status: 'running', round });
    const verify = await opts.api.verify(opts.session, extracted.toml, extracted.expects);
    if (stopped()) return { status: 'cancelled', rounds: round };
    // A draft that is not valid at all goes straight back. One that only fails the shared rules or the
    // fixture checks (a plan is present) is still tested on the real program, so the next round gets
    // the program's real output together with those problems.
    const planned = verify.planned;
    const plannedVisible = verifyVisiblePlan(verify, visibleMode);
    if (opts.autonomous && verify.errors.some((e) => e.code === 'WIZARD_AUTO_BYPASS_FLAG')) {
      opts.autonomous.onDecision('Refused a permission-bypass flag in the draft and asked the writer to remove it');
    }
    if (!verify.ok && !planned && !plannedVisible) {
      emit({ step: 'check', status: 'failed', round, detail: plural(verify.errors.length, 'problem') });
      lastText = verify.text;
      feedback = { previousToml: extracted.toml, previousExpects: extracted.expects, text: verify.text };
      continue;
    }
    if (verify.ok) {
      emit({ step: 'check', status: 'done', round });
    } else {
      emit({
        step: 'check',
        status: 'failed',
        round,
        detail: `${plural(verify.errors.length, 'problem')}, testing it on the real program anyway`,
      });
      lastText = verify.text;
    }
    if (!planned && !plannedVisible) throw new Error('WIZARD_PLAN_REQUIRED: the checker did not say which commands the tests would run.');

    let visible: VisibleTestResult | null = null;
    let live: LiveResult | null = null;
    const fail = (text: string, reported: AgentError | null): { stop: AgentError } | null => {
      // The program itself is broken (or says the same error twice): another draft cannot help. Only real error
      // text counts: a banner, a logo or an empty line is not one, and a failure of the draft goes back to the drafter.
      const agentError = realAgentError(reported);
      if (agentError && (agentError.external || agentError.message === lastAgentError)) return { stop: agentError };
      lastAgentError = agentError?.message ?? null;
      lastText = text;
      feedback = { previousToml: extracted.toml, previousExpects: extracted.expects, text };
      return null;
    };

    if (plannedVisible) {
      if (confirmedVisible !== plannedVisible.fingerprint) {
        if (opts.autonomous) {
          // Nobody confirms: the exact command is logged here and kept in <id>.wizard.json by Rust.
          opts.autonomous.onDecision(`Will run (visible test): ${commandLine(opts.facts.command, plannedVisible.args)}`);
        } else {
          const go = (await opts.confirmVisible?.(plannedVisible)) ?? false;
          if (!go) return { status: 'declined', toml: extracted.toml, expects: extracted.expects, rounds: round, verify };
        }
        confirmedVisible = plannedVisible.fingerprint;
      }
      if (stopped()) return { status: 'cancelled', rounds: round };
      emit({ step: 'visible', status: 'running', round });
      if (!opts.api.visibletest) throw new Error('WIZARD_NOT_AVAILABLE: this wizard cannot run the visible test.');
      const watcher = opts.autonomous?.watchTest?.(opts.session);
      try {
        visible = await opts.api.visibletest(opts.session, plannedVisible.fingerprint);
      } catch (e) {
        const err = splitError(e);
        if (opts.autonomous && err.code === 'WIZARD_AUTO_BYPASS_REFUSED') {
          // Rust refused to start the pane: the draft goes back with the reason.
          emit({ step: 'visible', status: 'failed', round, detail: err.message });
          opts.autonomous.onDecision('Refused a permission-bypass flag in the visible test and asked the writer to remove it');
          lastText = err.message;
          feedback = { previousToml: extracted.toml, previousExpects: extracted.expects, text: err.message };
          continue;
        }
        throw e;
      } finally {
        watcher?.stop();
      }
      if (stopped()) return { status: 'cancelled', rounds: round };
      const signIn = watcher?.signIn();
      if (signIn) return { status: 'sign_in', rounds: round, message: signIn };
      if (!visible.ok) {
        emit({ step: 'visible', status: 'failed', round, detail: visible.errors[0]?.message });
        const ae = visible.agent_error ? { stage: visible.agent_error.check, message: visible.agent_error.message, external: visible.agent_error.external } : null;
        const stop = fail(visible.text, ae);
        if (stop) return { status: 'agent_error', toml: extracted.toml, expects: extracted.expects, rounds: round, verify, live, visible, error: stop.stop };
        continue;
      }
      emit({ step: 'visible', status: 'done', round });
    }

    if (planned) {
      if (confirmedPlan !== planned.fingerprint) {
        if (opts.autonomous) {
          for (const stage of planned.stages) {
            opts.autonomous.onDecision(`Will run (one-task test, ${stage.label.toLowerCase()}): ${commandLine(opts.facts.command, stage.args)}`);
          }
        } else if (!(await opts.confirmLive?.(planned))) {
          return { status: 'declined', toml: extracted.toml, expects: extracted.expects, rounds: round, verify };
        }
        confirmedPlan = planned.fingerprint;
      }
      if (stopped()) return { status: 'cancelled', rounds: round };
      emit({ step: 'live', status: 'running', round });
      try {
        live = await opts.api.livetest(opts.session, planned.fingerprint);
      } catch (e) {
        const err = splitError(e);
        if (opts.autonomous && err.code === 'WIZARD_AUTO_BYPASS_REFUSED') {
          emit({ step: 'live', status: 'failed', round, detail: err.message });
          opts.autonomous.onDecision('Refused a permission-bypass flag in the one-task test and asked the writer to remove it');
          lastText = err.message;
          feedback = { previousToml: extracted.toml, previousExpects: extracted.expects, text: err.message };
          continue;
        }
        throw e;
      }
      if (stopped()) return { status: 'cancelled', rounds: round };
      if (!live.ok) {
        emit({ step: 'live', status: 'failed', round, detail: live.errors[0]?.message });
        samples = samplesFrom(live);
        const stop = fail(live.text, live.agent_error ?? null);
        if (stop) return { status: 'agent_error', toml: extracted.toml, expects: extracted.expects, rounds: round, verify, live, visible, error: stop.stop };
        continue;
      }
      emit({ step: 'live', status: 'done', round });
    }
    if (opts.autonomous && !verify.ok) {
      // Only a draft the checker passed with no problem at all is finished without a person: the tests passed, but the
      // checker's notes still go back to the writer.
      emit({ step: 'check', status: 'failed', round, detail: `${plural(verify.errors.length, 'problem')} left after the tests` });
      lastText = verify.text;
      feedback = { previousToml: extracted.toml, previousExpects: extracted.expects, text: verify.text };
      continue;
    }
    return { status: 'tested', toml: extracted.toml, expects: extracted.expects, rounds: round, verify, live, visible };
  }
  return { status: 'gave_up', rounds: round, lastToml, lastText };
}

/** The visible plan only counts in a session that captured screens (a headless-only session never has one). */
function verifyVisiblePlan(verify: VerifyResult, visibleMode: boolean): PlannedVisible | undefined {
  return visibleMode ? verify.planned_visible : undefined;
}

/** What to tell the person when the program itself returned an error (plain words, no profile jargon). */
export function agentErrorMessage(agent: string, message: string): { title: string; text: string } {
  const first = maskEmails(message.trim().split('\n')[0].replace(/[.\s]+$/, ''));
  return {
    title: `${agent} itself returned an error: ${first}.`,
    text: `This is a problem with ${agent}'s own setup (sign-in, model or provider), not with the profile. Fix it in ${agent}, then press Retry.`,
  };
}

export type RetryResult =
  | { status: 'tested'; toml: string; expects: Record<string, string>; rounds: number; verify: VerifyResult; live: LiveResult | null; visible: VisibleTestResult | null }
  | {
      status: 'agent_error';
      toml: string;
      expects: Record<string, string>;
      rounds: number;
      verify: VerifyResult;
      live: LiveResult | null;
      visible: VisibleTestResult | null;
      error: AgentError;
    }
  | { status: 'failed'; toml: string; expects: Record<string, string>; rounds: number; verify: VerifyResult; live: LiveResult | null; visible: VisibleTestResult | null; text: string };

/**
 * Retry after the program's own error: run only the tests again for the draft that is kept (same
 * commands, so the confirmation still holds). No new drafting.
 */
export async function retryLiveTest(opts: {
  api: Pick<WizardApi, 'livetest'> & Partial<Pick<WizardApi, 'visibletest'>>;
  session: string;
  toml: string;
  expects?: Record<string, string>;
  rounds: number;
  verify: VerifyResult;
  onEvent?: (event: ProgressEvent) => void;
}): Promise<RetryResult> {
  const planned = opts.verify.planned;
  const plannedVisible = opts.verify.planned_visible;
  if (!planned && !plannedVisible) throw new Error('WIZARD_PLAN_REQUIRED: there is no planned command to run again.');
  const expects = opts.expects ?? {};
  const base = { toml: opts.toml, expects, rounds: opts.rounds, verify: opts.verify };
  let visible: VisibleTestResult | null = null;
  let live: LiveResult | null = null;
  if (plannedVisible) {
    opts.onEvent?.({ step: 'visible', status: 'running', round: opts.rounds });
    if (!opts.api.visibletest) throw new Error('WIZARD_NOT_AVAILABLE: this wizard cannot run the visible test.');
    visible = await opts.api.visibletest(opts.session, plannedVisible.fingerprint);
    if (!visible.ok) {
      opts.onEvent?.({ step: 'visible', status: 'failed', round: opts.rounds, detail: visible.errors[0]?.message });
      const ae = visible.agent_error;
      if (ae?.external && isRealErrorMessage(ae.message)) return { status: 'agent_error', ...base, live, visible, error: { stage: ae.check, message: ae.message, external: true } };
      return { status: 'failed', ...base, live, visible, text: visible.text };
    }
    opts.onEvent?.({ step: 'visible', status: 'done', round: opts.rounds });
  }
  if (planned) {
    opts.onEvent?.({ step: 'live', status: 'running', round: opts.rounds });
    live = await opts.api.livetest(opts.session, planned.fingerprint);
    if (!live.ok) {
      opts.onEvent?.({ step: 'live', status: 'failed', round: opts.rounds, detail: live.errors[0]?.message });
      const ae = realAgentError(live.agent_error);
      if (ae?.external) return { status: 'agent_error', ...base, live, visible, error: ae };
      return { status: 'failed', ...base, live, visible, text: live.text };
    }
    opts.onEvent?.({ step: 'live', status: 'done', round: opts.rounds });
  }
  return { status: 'tested', ...base, live, visible };
}

/** The command line the person will approve: the program name and the argv with `<task>`. */
export function commandLine(command: string, argv: string[] | null | undefined): string {
  if (!argv) return command;
  return [command, ...argv].map((a) => (/^[\w./:=@%+,<>-]+$/.test(a) ? a : JSON.stringify(a))).join(' ');
}
