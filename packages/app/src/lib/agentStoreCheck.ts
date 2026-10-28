// "Check it works" of the Agent Store, the real run behind `StoreCheck.tsx` (SPEC-n32 3.6, decision 10).
//
// It runs the shipped catalog definition through the existing agent wizard as a report only: the wizard starts the agent in
// its own pane in its own temp folder, captures its screens, and its "first draft" is the catalog definition (a stand-in
// writer). Nothing is saved or approved (`finish` is never called, see `dryRun` in agentWizardAuto.ts), and only the numbers
// and the version go to the Store record. A sign-in screen is never answered: the run stops and says so.
//
// Generic code: no agent is named here. The agent's program name comes from the definition's `[binary]` table.
import { invoke } from '@tauri-apps/api/core';
import { agentStoreApi } from './agent-store';
import { splitError, tauriWizardApi, type ProgressEvent, type WizardApi } from './agentWizard';
import { isScreenLook, pickAutoDrafter, runAutonomous, type AutoTimings, type Clock } from './agentWizardAuto';
import type { RunStoreCheck, StoreCheckResult } from '../components/store/StoreCheck';

/** The error of a repair round when no Claude Code or Codex is installed to answer it. */
export const NO_DRAFTER = 'NO_DRAFTER';

export const NO_DRAFTER_MESSAGE =
  'The definition that comes with Puppet Master did not pass on this PC, and neither Claude Code nor Codex is installed to repair it. Nothing was changed.';

const FENCE = '`'.repeat(3);

/** One question to a real writer AI (the wizard's `ask` for the session); absent when none is installed. */
export type RealAsk = (prompt: string, round: number) => Promise<string>;

export interface StoreAskOptions {
  /** The shipped definition (`definition.toml`) of the catalog entry. */
  catalogToml: string;
  /** The entry's expectation files by capture name (`tui_idle` -> the text of `tui_idle.expect.toml`). Empty for a starter. */
  expects: Record<string, string>;
  realAsk?: RealAsk;
}

/** The first draft in the reply format the wizard reads: the whole file, then one `# expect: <capture>` block per capture. */
export function catalogReply(catalogToml: string, expects: Record<string, string>): string {
  const blocks = [`${FENCE}toml\n${catalogToml.trim()}\n${FENCE}`];
  for (const name of Object.keys(expects).sort()) {
    blocks.push(`${FENCE}toml\n# expect: ${name}\n${expects[name].trim()}\n${FENCE}`);
  }
  return `The definition that ships with Puppet Master.\n\n${blocks.join('\n\n')}`;
}

/**
 * The stand-in writer. Round 1 of the draft is the catalog definition. A screen look (the one-word look or the menu look)
 * goes to a real writer when there is one, else it is "unknown" (the wizard then waits and stops with the person's question).
 * A repair round goes to a real writer, else it throws `NO_DRAFTER` (the run stops and says so).
 */
export function makeStoreAsk(o: StoreAskOptions): (prompt: string, round: number) => Promise<string> {
  return async (prompt, round) => {
    if (isScreenLook(prompt)) return o.realAsk ? o.realAsk(prompt, round) : 'unknown';
    if (round <= 1) return catalogReply(o.catalogToml, o.expects);
    if (o.realAsk) return o.realAsk(prompt, round);
    throw new Error(`${NO_DRAFTER}: ${NO_DRAFTER_MESSAGE}`);
  };
}

// ---- reading the catalog entry --------------------------------------------------------------------------------------

/** What the Rust side sends for one entry: the shipped definition text and the fixture files (names and text only). */
export interface StoreCheckSource {
  /** `null` for a built-in agent: it has no definition to check. */
  definition: string | null;
  fixtures: Array<{ name: string; text: string }>;
}

const EXPECT_SUFFIX = '.expect.toml';

/** `tui_idle.expect.toml` -> `tui_idle`: the expectation files of the fixtures, by capture name. */
export function expectsFrom(fixtures: StoreCheckSource['fixtures']): Record<string, string> {
  const out: Record<string, string> = {};
  for (const f of fixtures) {
    if (f.name.endsWith(EXPECT_SUFFIX) && f.text.trim()) out[f.name.slice(0, -EXPECT_SUFFIX.length)] = f.text;
  }
  return out;
}

/** The first program name of the definition's `[binary]` table, or null. Only plain file-name characters are accepted. */
export function firstBinaryName(definition: string): string | null {
  const table = /^\s*\[binary\]\s*$([\s\S]*?)(?=^\s*\[|(?![\s\S]))/m.exec(definition);
  const names = table ? /^\s*names\s*=\s*\[\s*"([^"\r\n]+)"/m.exec(table[1]) : null;
  const name = names?.[1].trim() ?? '';
  return /^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/.test(name) ? name : null;
}

// ---- what the run needs ---------------------------------------------------------------------------------------------

export interface StoreCheckDeps {
  wizard: WizardApi;
  /** Reads the shipped definition and fixtures of a catalog id. */
  source: (id: string) => Promise<StoreCheckSource>;
  recordCheck: (id: string, passed: number, total: number, version: string) => Promise<void>;
  /** Tests only: the wizard's waits and clock. */
  timings?: Partial<AutoTimings>;
  clock?: Clock;
}

const realDeps: StoreCheckDeps = {
  wizard: tauriWizardApi,
  // Added with the other `agent_store_*` commands (AS-17): `{ definition: string | null, fixtures: [{ name, text }] }`.
  source: (id) => invoke<StoreCheckSource>('agent_store_check_source', { id }),
  recordCheck: agentStoreApi.recordCheck,
};

// ---- messages -------------------------------------------------------------------------------------------------------

/** A plain sentence for the sheet: no path, no control character, not long. */
export function plain(text: string, max = 300): string {
  const clean = text
    .replace(/[A-Za-z]:[\\/][^\s"')]*/g, '<path>')
    .replace(/(^|\s)~[\\/][^\s"')]*/g, '$1<path>')
    // eslint-disable-next-line no-control-regex
    .replace(/[\u0000-\u001f]+/g, ' ')
    .replace(/\s+/g, ' ')
    .trim();
  return clean.length > max ? `${clean.slice(0, max - 3).trimEnd()}...` : clean;
}

const STOPPED = 'Check stopped. Nothing was added or approved.';
const CLOSE_WAIT_MS = 3000;

/** Waits for a promise, but never longer than `ms`: the sheet must not hang on a slow close. */
function within(promise: Promise<unknown>, ms: number): Promise<void> {
  return new Promise((resolve) => {
    const timer = setTimeout(resolve, ms);
    promise.then(
      () => {
        clearTimeout(timer);
        resolve();
      },
      () => {
        clearTimeout(timer);
        resolve();
      },
    );
  });
}

// ---- the run --------------------------------------------------------------------------------------------------------

/**
 * Runs "Check it works" for one catalog id. Settles soon after `signal` aborts, whatever the wizard is doing. The wizard
 * session is always cancelled. Only a finished check is recorded (numbers and the version); a sign-in stop and a stop record
 * nothing.
 */
export async function runStoreCheck(
  id: string,
  signal: AbortSignal,
  onEvent: (event: ProgressEvent) => void,
  deps: StoreCheckDeps = realDeps,
): Promise<StoreCheckResult> {
  const api = deps.wizard;
  let session: string | null = null;
  let closing: Promise<void> | null = null;
  const closeSession = (): Promise<void> => {
    if (!session) return Promise.resolve();
    closing ??= within(Promise.resolve(api.cancel(session)), CLOSE_WAIT_MS);
    return closing;
  };

  const work = (async (): Promise<StoreCheckResult> => {
    const source = await deps.source(id);
    const command = source.definition ? firstBinaryName(source.definition) : null;
    if (!source.definition || !command) {
      return { status: 'stopped', message: 'This agent has no definition that can be checked.' };
    }
    if (signal.aborted) return { status: 'stopped', message: STOPPED };

    const start = await api.start(command, true, true);
    session = start.session;
    if (signal.aborted) return { status: 'stopped', message: STOPPED };
    const probe = await api.probe(start.session);
    const list = await Promise.resolve(api.drafters()).catch(() => []);
    const picked = pickAutoDrafter(list);
    const realAsk: RealAsk | undefined = picked ? (prompt) => api.ask(start.session, picked.id, prompt) : undefined;
    let noDrafter = false;
    const storeAsk = makeStoreAsk({ catalogToml: source.definition, expects: expectsFrom(source.fixtures), realAsk });
    const ask = async (prompt: string, round: number) => {
      try {
        return await storeAsk(prompt, round);
      } catch (e) {
        if (splitError(e).code === NO_DRAFTER) noDrafter = true;
        throw e;
      }
    };

    const outcome = await runAutonomous({
      api,
      start,
      probe,
      drafter: { kind: 'local_agent', id: picked?.id ?? 'catalog', label: picked?.label ?? 'the Store catalog' },
      ask,
      onEvent: (e) => {
        if (!signal.aborted) onEvent(e);
      },
      onDecision: () => undefined,
      signal,
      timings: deps.timings,
      clock: deps.clock,
      dryRun: true,
    });

    switch (outcome.status) {
      case 'tested': {
        if (signal.aborted) return { status: 'stopped', message: STOPPED };
        const version = outcome.version ?? '';
        let note: string | undefined;
        try {
          await deps.recordCheck(id, outcome.checks, outcome.total, version);
        } catch {
          note = 'The result could not be saved on the card.';
        }
        return { status: 'done', passed: outcome.checks, total: outcome.total, version: version || 'unknown', ...(note ? { note } : {}) };
      }
      case 'sign_in':
        return { status: 'sign_in', message: plain(outcome.message) };
      case 'cancelled':
        return { status: 'stopped', message: STOPPED };
      case 'stopped': {
        if (noDrafter) return { status: 'stopped', message: NO_DRAFTER_MESSAGE };
        if (outcome.paneOpen) {
          // A question only the person may answer: the check never picks an answer, and it keeps nothing of the screen.
          return {
            status: 'stopped',
            message: `${plain(outcome.title)} Open the agent yourself, answer it there, then press Retry. The check never answers it for you.`,
          };
        }
        // Only the title, plus the program's own first error line: the longer text can carry screens and drafts.
        const first = outcome.agentError ? plain(outcome.text.split('\n')[0] ?? '', 200) : '';
        return { status: 'stopped', message: `${plain(outcome.title)}${first ? ` ${first}` : ''} Nothing was added or approved.` };
      }
    }
  })();

  const aborted = new Promise<'aborted'>((resolve) => {
    if (signal.aborted) resolve('aborted');
    else signal.addEventListener('abort', () => resolve('aborted'), { once: true });
  });

  try {
    const result = await Promise.race([work, aborted]);
    return result === 'aborted' ? { status: 'stopped', message: STOPPED } : result;
  } catch (e) {
    return { status: 'stopped', message: signal.aborted ? STOPPED : plain(splitError(e).message) || 'The check could not finish.' };
  } finally {
    await closeSession();
    // A start that was still running when the abort came gives its session later: close that one too.
    void work.then(closeSession, () => undefined);
  }
}

/** The `RunStoreCheck` the Check sheet takes, for given dependencies (the real ones when omitted). */
export function makeRunStoreCheck(deps: StoreCheckDeps = realDeps): RunStoreCheck {
  return (id, signal, onEvent) => runStoreCheck(id, signal, onEvent, deps);
}
