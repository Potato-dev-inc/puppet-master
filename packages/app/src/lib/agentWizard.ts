// Agent Wizard front end (docs/specs/agent-wizard/SPEC.md): the typed API over the Tauri commands, the
// drafters, and the draft loop. Rust is deterministic and does no AI work; the loop that asks an AI for
// a definition lives here. It runs: build the prompt, ask the drafter, take exactly one ```toml block,
// verify, feed the checker's text back verbatim, then (once the person agrees) the live test, at most
// five rounds in all.
import { invoke } from '@tauri-apps/api/core';
import {
  buildPrompt,
  LOCAL_PROMPT_BUDGET,
  PROMPT_VERSION,
  REMOTE_PROMPT_BUDGET,
  type ProbeRun,
  type PromptFacts,
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

export interface VerifyResult {
  ok: boolean;
  state: string;
  errors: CheckError[];
  /** The errors written for an AI. Goes back into the next prompt verbatim. */
  text: string;
  warnings: string[];
  rendered?: { headless_argv: string[] | null; resume_argv: string[] | null };
  checked: { fixtures: number; rules: number };
  samples_replayed: number;
  sha256?: string;
  /**
   * What the live test would run for this draft. Present when `ok`, and also when the draft only fails
   * the shared rules or the fixture checks (`can_livetest`); absent for a draft that is not valid.
   */
  planned?: PlannedRun;
  can_livetest?: boolean;
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

export interface Provenance {
  drafter: string;
  drafter_kind: 'local_agent' | 'openrouter' | 'other';
  model: string | null;
  prompt_version: string;
  rounds: number;
}

export interface FinishResult {
  ok: boolean;
  id: string;
  file: string;
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
  start(command: string): Promise<StartInfo>;
  probe(session: string): Promise<ProbeInfo>;
  verify(session: string, toml: string): Promise<VerifyResult>;
  /** Runs the live test. `plan` is the fingerprint of the commands the person confirmed. */
  livetest(session: string, plan: string): Promise<LiveResult>;
  finish(session: string, provenance: Provenance): Promise<FinishResult>;
  cancel(session: string): Promise<unknown>;
  /** One read-only question to an installed local agent, in the session's work folder. */
  ask(session: string, agent: string, prompt: string): Promise<string>;
  /** Installed local agents that can answer a read-only question. */
  drafters(): Promise<LocalDrafter[]>;
}

export const tauriWizardApi: WizardApi = {
  start: (command) => invoke<StartInfo>('agent_wizard_start', { command }),
  probe: (session) => invoke<ProbeInfo>('agent_wizard_probe', { session }),
  verify: (session, toml) => invoke<VerifyResult>('agent_wizard_verify', { session, toml }),
  livetest: (session, plan) => invoke<LiveResult>('agent_wizard_livetest', { session, confirm: true, plan }),
  finish: (session, provenance) => invoke<FinishResult>('agent_wizard_finish', { session, provenance }),
  cancel: (session) => invoke('agent_wizard_cancel', { session }),
  ask: async (session, agent, prompt) => (await invoke<{ answer: string }>('agent_wizard_ask', { session, agent, prompt })).answer,
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
    return askOpenRouter({ apiKey: key, model, prompt, signal });
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

// ---- facts for the prompt --------------------------------------------------------------------

export function factsFrom(start: StartInfo, probe: ProbeInfo): PromptFacts {
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

export type StepId = 'help' | 'draft' | 'check' | 'fix' | 'live';

export interface ProgressEvent {
  step: StepId;
  status: 'running' | 'done' | 'failed';
  round: number;
  detail?: string;
}

export type LoopResult =
  | { status: 'tested'; toml: string; rounds: number; verify: VerifyResult; live: LiveResult }
  | { status: 'declined'; toml: string; rounds: number; verify: VerifyResult }
  | { status: 'agent_error'; toml: string; rounds: number; verify: VerifyResult; live: LiveResult; error: AgentError }
  | { status: 'gave_up'; rounds: number; lastToml: string; lastText: string }
  | { status: 'cancelled'; rounds: number };

export interface LoopOptions {
  api: Pick<WizardApi, 'verify' | 'livetest'>;
  session: string;
  facts: PromptFacts;
  /** Sends one prompt to the drafter and returns its reply. */
  ask: (prompt: string, round: number) => Promise<string>;
  maxChars: number;
  maxRounds?: number;
  /**
   * Called before a live test whose planned commands differ from the ones already confirmed (so: the
   * first time, and again when a fix changes the arguments). `false` stops the loop with `declined`.
   */
  confirmLive: (planned: PlannedRun) => Promise<boolean>;
  onEvent?: (event: ProgressEvent) => void;
  signal?: AbortSignal;
}

export async function runDraftLoop(opts: LoopOptions): Promise<LoopResult> {
  const max = opts.maxRounds ?? MAX_ROUNDS;
  const emit = (e: ProgressEvent) => opts.onEvent?.(e);
  let feedback: { previousToml: string; text: string } | undefined;
  let samples: Sample[] = [];
  let lastToml = '';
  let lastText = '';
  let confirmedPlan: string | null = null;
  let lastAgentError: string | null = null;
  let round = 0;
  const stopped = () => opts.signal?.aborted === true;

  while (round < max) {
    round += 1;
    if (stopped()) return { status: 'cancelled', rounds: round - 1 };
    const writing: StepId = round === 1 ? 'draft' : 'fix';
    emit({ step: writing, status: 'running', round });
    const prompt = buildPrompt({ ...opts.facts, samples }, { maxChars: opts.maxChars, feedback });
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

    const extracted = extractTomlBlock(reply);
    if (!extracted.ok) {
      emit({ step: 'check', status: 'failed', round, detail: extracted.problem });
      lastText = extracted.problem;
      feedback = { previousToml: lastToml, text: extracted.problem };
      continue;
    }
    lastToml = extracted.toml;

    emit({ step: 'check', status: 'running', round });
    const verify = await opts.api.verify(opts.session, extracted.toml);
    if (stopped()) return { status: 'cancelled', rounds: round };
    // A draft that is not valid at all goes straight back. One that only fails the shared rules or the
    // fixture checks (`planned` is present) is still tested on the real program, so the next round gets
    // the program's real output together with those problems.
    const planned = verify.planned;
    if (!verify.ok && !planned) {
      emit({ step: 'check', status: 'failed', round, detail: `${verify.errors.length} problem${verify.errors.length === 1 ? '' : 's'}` });
      lastText = verify.text;
      feedback = { previousToml: extracted.toml, text: verify.text };
      continue;
    }
    if (verify.ok) {
      emit({ step: 'check', status: 'done', round });
    } else {
      emit({
        step: 'check',
        status: 'failed',
        round,
        detail: `${verify.errors.length} problem${verify.errors.length === 1 ? '' : 's'}, testing it on the real program anyway`,
      });
      lastText = verify.text;
    }
    if (!planned) throw new Error('WIZARD_PLAN_REQUIRED: the checker did not say which commands the live test would run.');
    if (confirmedPlan !== planned.fingerprint) {
      if (!(await opts.confirmLive(planned))) return { status: 'declined', toml: extracted.toml, rounds: round, verify };
      confirmedPlan = planned.fingerprint;
    }
    if (stopped()) return { status: 'cancelled', rounds: round };
    emit({ step: 'live', status: 'running', round });
    const live = await opts.api.livetest(opts.session, planned.fingerprint);
    if (stopped()) return { status: 'cancelled', rounds: round };
    if (live.ok) {
      emit({ step: 'live', status: 'done', round });
      return { status: 'tested', toml: extracted.toml, rounds: round, verify, live };
    }
    emit({ step: 'live', status: 'failed', round, detail: live.errors[0]?.message });
    // The program itself is broken (or says the same error twice): another draft cannot help.
    const agentError = live.agent_error ?? null;
    if (agentError && (agentError.external || agentError.message === lastAgentError)) {
      return { status: 'agent_error', toml: extracted.toml, rounds: round, verify, live, error: agentError };
    }
    lastAgentError = agentError?.message ?? null;
    lastText = live.text;
    samples = samplesFrom(live);
    feedback = { previousToml: extracted.toml, text: live.text };
  }
  return { status: 'gave_up', rounds: round, lastToml, lastText };
}

/** What to tell the person when the program itself returned an error (plain words, no profile jargon). */
export function agentErrorMessage(agent: string, message: string): { title: string; text: string } {
  const first = message.trim().split('\n')[0].replace(/[.\s]+$/, '');
  return {
    title: `${agent} itself returned an error: ${first}.`,
    text: `This is a problem with ${agent}'s own setup (sign-in, model or provider), not with the profile. Fix it in ${agent}, then press Retry.`,
  };
}

export type RetryResult =
  | { status: 'tested'; toml: string; rounds: number; verify: VerifyResult; live: LiveResult }
  | { status: 'agent_error'; toml: string; rounds: number; verify: VerifyResult; live: LiveResult; error: AgentError }
  | { status: 'failed'; toml: string; rounds: number; verify: VerifyResult; live: LiveResult };

/**
 * Retry after the program's own error: run only the live test again for the draft that is kept (same
 * commands, so the confirmation still holds). No new drafting.
 */
export async function retryLiveTest(opts: {
  api: Pick<WizardApi, 'livetest'>;
  session: string;
  toml: string;
  rounds: number;
  verify: VerifyResult;
  onEvent?: (event: ProgressEvent) => void;
}): Promise<RetryResult> {
  const planned = opts.verify.planned;
  if (!planned) throw new Error('WIZARD_PLAN_REQUIRED: there is no planned command to run again.');
  opts.onEvent?.({ step: 'live', status: 'running', round: opts.rounds });
  const live = await opts.api.livetest(opts.session, planned.fingerprint);
  const base = { toml: opts.toml, rounds: opts.rounds, verify: opts.verify, live };
  if (live.ok) {
    opts.onEvent?.({ step: 'live', status: 'done', round: opts.rounds });
    return { status: 'tested', ...base };
  }
  opts.onEvent?.({ step: 'live', status: 'failed', round: opts.rounds, detail: live.errors[0]?.message });
  if (live.agent_error?.external) return { status: 'agent_error', ...base, error: live.agent_error };
  return { status: 'failed', ...base };
}

/** The command line the person will approve: the program name and the argv with `<task>`. */
export function commandLine(command: string, argv: string[] | null | undefined): string {
  if (!argv) return command;
  return [command, ...argv].map((a) => (/^[\w./:=@%+,<>-]+$/.test(a) ? a : JSON.stringify(a))).join(' ');
}
