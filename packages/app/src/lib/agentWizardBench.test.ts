// Benchmark loop for the Agent Wizard without the GUI (docs/specs/agent-wizard/SPEC.md, "Benchmark").
//
// The loop, per round N in <dir>/round-N/:
//   1. PROMPT   build the prompt with the app's own builder        -> prompt.txt
//   2. (you)    claude -p < prompt.txt > reply.md                    (any drafter works)
//   3. EXTRACT  take the one ```toml block of reply.md              -> draft.toml
//   4. (Rust)   `bench_round` verifies the draft and runs the live test
//                                                                    -> verify.txt, live.json, samples.json,
//                                                                       feedback.json, result.txt
//   round N+1's PROMPT reads round N's feedback.json (the checker text, the previous draft and the real
//   output of the live test, with the answers masked), exactly as the draft loop in lib/agentWizard.ts does.
//
// The visible path (SPEC 11.13) has the same loop. `<dir>/captures.json` (written once by the `wizard_bench_capture`
// test against the real program) makes the prompt a visible one: the captured screens, masked, are in it; the reply
// may carry `# expect: <capture>` blocks, which the extract step keeps in `round-N/expects.json`; `wizard_bench_round`
// verifies the draft and runs the visible test on the real program. Without `captures.json` everything is as before.
//
// The two steps below run only with PM_WIZARD_BENCH=1:
//   PM_WIZARD_BENCH=1 PM_WIZARD_BENCH_STEP=prompt|extract PM_WIZARD_BENCH_DIR=<dir> PM_WIZARD_BENCH_ROUND=<n>
//   [PM_WIZARD_BENCH_OMIT=kilo|kiro]   leave that agent's example out of the prompt (it is the one being rebuilt)
// The helpers are also tested in the normal suite against a temp folder.
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { extractDraft, extractTomlBlock, factsFrom, type ProbeInfo, type StartInfo } from './agentWizard';
import { buildPrompt, LOCAL_PROMPT_BUDGET, type CaptureFacts, type Sample } from './agentWizardPrompt';

interface Feedback {
  previous_toml: string;
  /** The visible rounds: the expectation blocks of the previous reply, by capture name. */
  previous_expects?: Record<string, string>;
  text: string;
  samples: Sample[];
}

/** The captured screens of the visible benchmark (masked: what the drafter sees), when a capture was made. */
function readCaptures(dir: string): CaptureFacts | null {
  const path = join(dir, 'captures.json');
  if (!existsSync(path)) return null;
  return readJson<{ masked: CaptureFacts }>(path).masked;
}

const roundDir = (dir: string, round: number) => join(dir, `round-${round}`);

function readJson<T>(path: string): T {
  return JSON.parse(readFileSync(path, 'utf8')) as T;
}

/** Step 1: write `round-N/prompt.txt`. Returns what happened, for the log. */
export function promptStep(dir: string, round: number, omit?: 'kilo' | 'kiro'): string {
  const probePath = join(dir, 'probe.json');
  if (!existsSync(probePath)) throw new Error(`${probePath} is missing: run the probe_dump test first`);
  const dump = readJson<{ start: StartInfo; probe: ProbeInfo }>(probePath);
  let feedback: Feedback | null = null;
  if (round > 1) {
    const previous = roundDir(dir, round - 1);
    const result = join(previous, 'result.txt');
    const last = existsSync(result) ? readFileSync(result, 'utf8') : '';
    if (last.startsWith('PASS')) return `round ${round - 1} already passed: nothing to do`;
    if (last.startsWith('STOP')) return `round ${round - 1} stopped (${last.trim()}): the agent itself is broken, a new draft will not help`;
    const path = join(previous, 'feedback.json');
    if (!existsSync(path)) throw new Error(`${path} is missing: finish round ${round - 1} first`);
    feedback = readJson<Feedback>(path);
  }
  const facts = { ...factsFrom(dump.start, dump.probe, readCaptures(dir)), samples: feedback?.samples ?? [] };
  const built = buildPrompt(facts, {
    maxChars: LOCAL_PROMPT_BUDGET,
    omitExample: omit,
    feedback: feedback ? { previousToml: feedback.previous_toml, previousExpects: feedback.previous_expects, text: feedback.text } : undefined,
  });
  const out = roundDir(dir, round);
  mkdirSync(out, { recursive: true });
  writeFileSync(join(out, 'prompt.txt'), built.text);
  return `wrote ${join(out, 'prompt.txt')}: ${built.chars} chars, cut: ${built.cut.join(', ') || 'none'}${built.overBudget ? ', OVER BUDGET' : ''}`;
}

/** Step 3: take the toml block of `round-N/reply.md` into `draft.toml`; a bad reply becomes feedback. */
export function extractStep(dir: string, round: number): string {
  const out = roundDir(dir, round);
  const reply = readFileSync(join(out, 'reply.md'), 'utf8');
  const visible = existsSync(join(dir, 'captures.json'));
  const got = visible ? extractDraft(reply) : extractTomlBlock(reply);
  if (got.ok) {
    writeFileSync(join(out, 'draft.toml'), `${got.toml}\n`);
    if (visible) writeFileSync(join(out, 'expects.json'), JSON.stringify('expects' in got ? got.expects : {}, null, 2));
    return `wrote ${join(out, 'draft.toml')} (${got.toml.length} chars)`;
  }
  const feedback: Feedback = { previous_toml: '', text: got.problem, samples: [] };
  writeFileSync(join(out, 'feedback.json'), JSON.stringify(feedback, null, 2));
  writeFileSync(join(out, 'result.txt'), `FAIL no-toml: ${got.problem}\n`);
  return `no usable toml block: ${got.problem}`;
}

describe.skipIf(!process.env.PM_WIZARD_BENCH)('wizard benchmark step (PM_WIZARD_BENCH=1)', () => {
  it('runs the step named by PM_WIZARD_BENCH_STEP', () => {
    const dir = process.env.PM_WIZARD_BENCH_DIR;
    const step = process.env.PM_WIZARD_BENCH_STEP;
    const round = Number(process.env.PM_WIZARD_BENCH_ROUND ?? '1');
    const omit = process.env.PM_WIZARD_BENCH_OMIT as 'kilo' | 'kiro' | undefined;
    if (!dir || !step || !Number.isInteger(round) || round < 1) throw new Error('set PM_WIZARD_BENCH_DIR, PM_WIZARD_BENCH_STEP and PM_WIZARD_BENCH_ROUND');
    const message = step === 'prompt' ? promptStep(dir, round, omit) : step === 'extract' ? extractStep(dir, round) : '';
    if (!message) throw new Error(`PM_WIZARD_BENCH_STEP must be prompt or extract, not ${step}`);
    console.log(message);
  });
});

// ---- the helpers, in the normal suite -----------------------------------------------------------

const start: StartInfo = {
  session: 'wz_1',
  state: 'resolved',
  command: 'fake-agent',
  id_suggestion: 'fake_agent',
  program: { path: 'C:\\x\\fake-agent.exe', display: '~\\fake-agent.exe', kind: 'native' },
  version: { ok: true, args: ['--version'], text: 'fake-agent 1.4.2', number: '1.4.2' },
  native_candidates: [],
  cost_note: '',
  limits: {},
};
const probe: ProbeInfo = {
  state: 'probed',
  runs: [{ label: '--help', args: ['--help'], text: 'Usage: fake-agent <COMMAND>', exit: 0, timed_out: false, truncated: false, bytes: 27, duplicate_of: null }],
  subcommands: [],
  skipped: [],
};

describe('benchmark helpers', () => {
  const F = '`'.repeat(3);

  function folder() {
    const dir = mkdtempSync(join(tmpdir(), 'pm-wizard-bench-'));
    writeFileSync(join(dir, 'probe.json'), JSON.stringify({ start, probe }));
    return dir;
  }

  it('round 1 builds a prompt from the probe; leave-one-out drops the example', () => {
    const dir = folder();
    try {
      expect(promptStep(dir, 1)).toContain('prompt.txt');
      const full = readFileSync(join(dir, 'round-1', 'prompt.txt'), 'utf8');
      expect(full).toContain('Command typed by the person: fake-agent');
      expect(full).toContain('Now write the file.');
      expect(full).toContain('runFinished');
      promptStep(dir, 1, 'kiro');
      expect(readFileSync(join(dir, 'round-1', 'prompt.txt'), 'utf8')).not.toContain('runFinished');
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it('extract takes the toml block, and a reply without one becomes the next round feedback', () => {
    const dir = folder();
    try {
      mkdirSync(join(dir, 'round-1'));
      writeFileSync(join(dir, 'round-1', 'reply.md'), `Here:\n${F}toml\nschema_version = 1\n${F}\n`);
      expect(extractStep(dir, 1)).toContain('draft.toml');
      expect(readFileSync(join(dir, 'round-1', 'draft.toml'), 'utf8')).toBe('schema_version = 1\n');

      mkdirSync(join(dir, 'round-2'));
      writeFileSync(join(dir, 'round-2', 'reply.md'), 'I could not do it.');
      expect(extractStep(dir, 2)).toContain('no usable toml block');
      expect(readFileSync(join(dir, 'round-2', 'result.txt'), 'utf8')).toMatch(/^FAIL no-toml/);
      promptStep(dir, 3);
      const prompt = readFileSync(join(dir, 'round-3', 'prompt.txt'), 'utf8');
      expect(prompt).toContain('Your reply had no ```toml block');
      expect(prompt).not.toContain('Your previous file');
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it('the next prompt carries the checker text, the previous draft and the real output', () => {
    const dir = folder();
    try {
      mkdirSync(join(dir, 'round-1'));
      const feedback: Feedback = {
        previous_toml: 'schema_version = 1\n# DRAFT ONE\n',
        text: 'The live test ran your file against the real program and found 1 problem.',
        samples: [{ stage: 'pong', args: ['run'], exit: 0, stdout: '{"event":"end","text":"<word-1>"}', stderr: '', result: null, session: null, error_code: 'AGENT_OUTPUT_UNPARSEABLE' }],
      };
      writeFileSync(join(dir, 'round-1', 'feedback.json'), JSON.stringify(feedback));
      writeFileSync(join(dir, 'round-1', 'result.txt'), 'FAIL livetest\n');
      expect(promptStep(dir, 2)).toContain('prompt.txt');
      const prompt = readFileSync(join(dir, 'round-2', 'prompt.txt'), 'utf8');
      expect(prompt).toContain('# DRAFT ONE');
      expect(prompt).toContain('found 1 problem');
      expect(prompt).toContain('<<<SAMPLE pong');
      expect(prompt).toContain('"text":"<word-1>"');
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it('stops when the previous round hit an error of the agent itself', () => {
    const dir = folder();
    try {
      mkdirSync(join(dir, 'round-1'));
      writeFileSync(join(dir, 'round-1', 'result.txt'), 'STOP agent-error\n');
      const message = promptStep(dir, 2);
      expect(message).toContain('stopped');
      expect(message).toContain('the agent itself is broken');
      expect(existsSync(join(dir, 'round-2', 'prompt.txt'))).toBe(false);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it('stops when the previous round passed, and says what is missing otherwise', () => {
    const dir = folder();
    try {
      mkdirSync(join(dir, 'round-1'));
      writeFileSync(join(dir, 'round-1', 'result.txt'), 'PASS\n');
      expect(promptStep(dir, 2)).toContain('already passed');
      expect(existsSync(join(dir, 'round-2', 'prompt.txt'))).toBe(false);
      expect(() => promptStep(dir, 3)).toThrow('feedback.json is missing');
      rmSync(join(dir, 'probe.json'));
      expect(() => promptStep(dir, 1)).toThrow('probe_dump');
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});

describe('benchmark helpers: the visible rounds', () => {
  const F = '`'.repeat(3);
  const masked: CaptureFacts = {
    grid: { cols: 120, rows: 40 },
    files: [
      { name: 'tui_idle', text: '| > Type a task...', variants: [] },
      { name: 'tui_reply', text: 'reply: <word-1>', variants: [] },
    ],
    facts: { exit_method: 'slash_exit', partial: [], skipped: [], hello_written: false, startup_dialogs: ['trust'], exit_screen: null, notes: [] },
  };

  function folder() {
    const dir = mkdtempSync(join(tmpdir(), 'pm-wizard-bench-visible-'));
    writeFileSync(join(dir, 'probe.json'), JSON.stringify({ start, probe }));
    // what `wizard_bench_capture` writes: the real screens stay in `raw`, the drafter only ever gets `masked`
    writeFileSync(join(dir, 'captures.json'), JSON.stringify({ command: 'fake-agent', words: { one: 'pong-secret', two: 'pong-other', sky: 'skyword' }, raw: { frames: [] }, masked }));
    return dir;
  }

  it('a folder with captures.json makes a visible prompt from the masked screens, and leave-one-out drops Kilo', () => {
    const dir = folder();
    try {
      expect(promptStep(dir, 1, 'kilo')).toContain('prompt.txt');
      const text = readFileSync(join(dir, 'round-1', 'prompt.txt'), 'utf8');
      expect(text).toContain('("visible mode")');
      expect(text).toContain('<<<CAPTURE tui_idle');
      expect(text).toContain('reply: <word-1>');
      expect(text).not.toContain('pong-secret');
      expect(text).not.toContain('ask anything'); // Kilo's [tui.ready] is left out, Nimbus stays
      expect(text).toContain('Nimbus, a made-up agent');
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it('extract keeps the expectation blocks next to the draft, and the next prompt shows them again', () => {
    const dir = folder();
    try {
      mkdirSync(join(dir, 'round-1'));
      const reply = `${F}toml\nschema_version = 1\n${F}\n${F}toml\n# expect: tui_idle\nchrome_lines = ["NIMBUS"]\n${F}\n`;
      writeFileSync(join(dir, 'round-1', 'reply.md'), reply);
      expect(extractStep(dir, 1)).toContain('draft.toml');
      expect(readFileSync(join(dir, 'round-1', 'draft.toml'), 'utf8')).toBe('schema_version = 1\n');
      expect(JSON.parse(readFileSync(join(dir, 'round-1', 'expects.json'), 'utf8'))).toEqual({ tui_idle: 'chrome_lines = ["NIMBUS"]' });
      // what `wizard_bench_round` writes when the visible test fails
      const feedback: Feedback = {
        previous_toml: 'schema_version = 1\n# DRAFT ONE\n',
        previous_expects: { tui_idle: 'chrome_lines = ["NIMBUS"]' },
        text: 'The visible test started your profile in a real pane and found 1 problem.',
        samples: [],
      };
      writeFileSync(join(dir, 'round-1', 'feedback.json'), JSON.stringify(feedback));
      writeFileSync(join(dir, 'round-1', 'result.txt'), 'FAIL visibletest\n');
      promptStep(dir, 2);
      const prompt = readFileSync(join(dir, 'round-2', 'prompt.txt'), 'utf8');
      expect(prompt).toContain('# DRAFT ONE');
      expect(prompt).toContain('# expect: tui_idle');
      expect(prompt).toContain('found 1 problem');
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it('a reply without a definition becomes feedback, as in the headless rounds', () => {
    const dir = folder();
    try {
      mkdirSync(join(dir, 'round-1'));
      writeFileSync(join(dir, 'round-1', 'reply.md'), 'I cannot do that.');
      expect(extractStep(dir, 1)).toContain('no usable toml block');
      expect(readFileSync(join(dir, 'round-1', 'result.txt'), 'utf8')).toMatch(/^FAIL no-toml/);
      expect(existsSync(join(dir, 'round-1', 'expects.json'))).toBe(false);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});
