import { describe, expect, it } from 'vitest';
import { CLIP_COLORS, CLIP_WIDTH, drawFrame, layoutTerminal, type Ctx2D } from './clipDraw';
import { GuardCounter } from './clipGuard';
import { buildTimeline, type Timeline } from './clipTimeline';
import type { ClipPlan, Frame, Replay, Run, Step, TerminalReplay } from './clipTypes';

// A fake secret is built here at run time, never written out as literal text.
const SECRET = `sk-ant-api03-${'A1'.repeat(10)}`;

interface Drawn {
  text: string;
  x: number;
  y: number;
  fill: string;
  font: string;
  alpha: number;
}

interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
  fill: string;
  alpha: number;
}

const pxOf = (font: string) => Number(/(\d+(?:\.\d+)?)px/.exec(font)?.[1] ?? 10);

/** A fake 2D context that records what is drawn. Text is 0.6 em per character, like a monospace font. */
function fakeCtx() {
  const texts: Drawn[] = [];
  const boxes: Box[] = [];
  const ctx = {
    fillStyle: '#000000',
    font: '10px sans-serif',
    textAlign: 'left',
    textBaseline: 'alphabetic',
    globalAlpha: 1,
    fillRect(x: number, y: number, w: number, h: number) {
      boxes.push({ x, y, w, h, fill: this.fillStyle, alpha: this.globalAlpha });
    },
    fillText(text: string, x: number, y: number) {
      texts.push({ text, x, y, fill: this.fillStyle, font: this.font, alpha: this.globalAlpha });
    },
    measureText(text: string) {
      return { width: text.length * 0.6 * pxOf(this.font) };
    },
    beginPath() {},
    moveTo() {},
    arcTo() {},
    closePath() {},
    fill() {},
    rect() {},
    clip() {},
    save() {},
    restore() {},
  };
  return { ctx: ctx as unknown as Ctx2D, texts, boxes };
}

function render(plan: ClipPlan, at: number, tl: Timeline = buildTimeline(plan)) {
  const { ctx, texts, boxes } = fakeCtx();
  const guard = new GuardCounter();
  drawFrame(ctx, plan, tl, at, { guard });
  return { strings: texts.map((d) => d.text), texts, boxes, guard };
}

function terminal(frames: Frame[], rowTable: Run[][], extra: Partial<TerminalReplay> = {}): TerminalReplay {
  return { kind: 'terminal', cols: 80, rows: 1, rowFrom: 0, rowTo: 0, speedX: 40, lengthMs: 6000, rowTable, frames, ...extra };
}

const step = (atMs: number, name: string, summary: string, extra: Partial<Step> = {}): Step => ({
  atMs,
  name,
  summary,
  paths: [],
  writes: false,
  helper: false,
  ok: true,
  ...extra,
});

function plan(extra: Partial<ClipPlan> = {}): ClipPlan {
  const rowTable: Run[][] = [[{ t: 'first row', s: 0 }]];
  return {
    token: 'tok',
    agent: 'Test Agent',
    request: 'Fix the login page',
    turnMs: 60_000,
    outcome: 'completed',
    replay: terminal([{ atMs: 0, rows: [0], cursor: null }], rowTable),
    steps: [],
    result: { files: null, checks: null, sentence: 'Done.' },
    notes: [],
    hidden: 0,
    ...extra,
  };
}

/** The clip time where the replay scene starts (after the 3 s title). */
const REPLAY_AT = 3_000;

describe('layoutTerminal', () => {
  it('keeps the font between 9 and 20 px and fits a normal terminal without cropping', () => {
    const r = layoutTerminal(80, 24);
    expect(r.fontPx).toBeCloseTo(520 / (24 * 1.25), 5);
    expect(r.crop).toBe(false);
  });

  it('caps the font at 20 px for a small screen', () => {
    expect(layoutTerminal(10, 1).fontPx).toBe(20);
  });

  it('floors the font at 9 px and crops the right side for a very wide screen', () => {
    const r = layoutTerminal(300, 24);
    expect(r.fontPx).toBe(9);
    expect(r.crop).toBe(true);
  });
});

describe('drawFrame scenes', () => {
  it('draws the title with the agent, the request and the small mark', () => {
    const r = render(plan(), 500);
    expect(r.strings).toContain('Test Agent');
    expect(r.strings).toContain('Fix the login page');
    expect(r.strings).toContain('Puppet Master');
  });

  it('draws the request from the timeline override only', () => {
    const p = plan();
    const r = render(p, 500, buildTimeline(p, 'Make the tests pass'));
    expect(r.strings).toContain('Make the tests pass');
    expect(r.strings).not.toContain('Fix the login page');
  });

  it('changes the terminal frame with the clip time', () => {
    const rowTable: Run[][] = [[{ t: 'first row', s: 0 }], [{ t: 'second row', s: 0 }]];
    const frames: Frame[] = [
      { atMs: 0, rows: [0], cursor: null },
      { atMs: 1000, rows: [1], cursor: null },
    ];
    const p = plan({ replay: terminal(frames, rowTable) });
    expect(render(p, REPLAY_AT + 500).strings).toContain('first row');
    expect(render(p, REPLAY_AT + 500).strings).not.toContain('second row');
    expect(render(p, REPLAY_AT + 1500).strings).toContain('second row');
  });

  it('draws the terminal text at the font size of the layout', () => {
    const r = render(plan(), REPLAY_AT + 10);
    const row = r.texts.find((d) => d.text === 'first row');
    expect(row?.font).toContain(`${layoutTerminal(80, 1).fontPx}px`);
    expect(row?.x).toBe(40);
    expect(row?.y).toBe(48);
  });

  it('draws the caret at its cell', () => {
    const frames: Frame[] = [{ atMs: 0, rows: [0], cursor: [0, 3] }];
    const r = render(plan({ replay: terminal(frames, [[{ t: 'abc', s: 0 }]]) }), REPLAY_AT + 10);
    const cellW = layoutTerminal(80, 1).fontPx * 0.6;
    expect(r.boxes.some((b) => b.x === 40 + 3 * cellW && b.w === 2 && b.fill === CLIP_COLORS.text)).toBe(true);
  });

  it('applies the style bits: highlight, inverse, dim', () => {
    const rowTable: Run[][] = [[{ t: 'a', s: 16 }, { t: 'b', s: 4 }, { t: 'c', s: 2 }]];
    const r = render(plan({ replay: terminal([{ atMs: 0, rows: [0], cursor: null }], rowTable) }), REPLAY_AT + 10);
    expect(r.boxes.some((b) => b.fill === CLIP_COLORS.raised)).toBe(true);
    expect(r.boxes.some((b) => b.fill === CLIP_COLORS.text)).toBe(true);
    expect(r.texts.find((d) => d.text === 'b')?.fill).toBe(CLIP_COLORS.bg);
    expect(r.texts.find((d) => d.text === 'c')?.fill).toBe(CLIP_COLORS.muted);
  });

  it('shows the sped-up badge only during the replay', () => {
    const p = plan();
    expect(render(p, 500).strings).not.toContain('Sped up 40x');
    expect(render(p, REPLAY_AT + 500).strings).toContain('Sped up 40x');
  });

  it('shows the caption pill of the timeline during its time', () => {
    const p = plan({ steps: [step(100, 'Read', 'a.ts'), step(200, 'Read', 'b.ts')] });
    const tl = buildTimeline(p);
    const caption = tl.captions.find((c) => c.role === 'caption');
    expect(caption).toBeDefined();
    expect(render(p, caption!.startMs + 10, tl).strings).toContain(caption!.text);
  });

  it('draws activity lines one by one and keeps only the newest 14', () => {
    const steps = Array.from({ length: 16 }, (_, i) => step(i * 900, 'Run', `step ${i}`));
    const p = plan({ replay: { kind: 'activity', lengthMs: 18_000 } as Replay, steps });
    expect(render(p, REPLAY_AT + 0).strings).toContain('step 0');
    expect(render(p, REPLAY_AT + 0).strings).not.toContain('step 1');
    const full = render(p, REPLAY_AT + 17_999).strings;
    expect(full).toContain('step 15');
    expect(full).toContain('step 2');
    expect(full).not.toContain('step 1');
    expect(full).not.toContain('step 0');
  });

  it('marks a failed step with the fixed label', () => {
    const p = plan({ replay: { kind: 'activity', lengthMs: 4000 } as Replay, steps: [step(0, 'Run', 'npm test', { ok: false })] });
    expect(render(p, REPLAY_AT + 10).strings).toContain('failed');
  });

  it('draws the result with the file counts in two tones and the names and checks', () => {
    const p = plan({
      result: {
        files: { count: 4, added: 86, deleted: 12, names: ['a.ts', 'b.ts'] },
        checks: 'all checks passed (2/2)',
        sentence: 'Done.',
      },
    });
    const tl = buildTimeline(p);
    const result = tl.scenes.find((s) => s.kind === 'result')!;
    const r = render(p, result.startMs + 100, tl);
    expect(r.strings).toContain('4 files changed');
    expect(r.strings).toContain('a.ts');
    expect(r.strings).toContain('b.ts');
    expect(r.strings).toContain('all checks passed (2/2)');
    expect(r.texts.find((d) => d.text === '+86')?.fill).toBe(CLIP_COLORS.text);
    expect(r.texts.find((d) => d.text === '-12')?.fill).toBe(CLIP_COLORS.muted);
  });

  it('uses the singular for one file', () => {
    const p = plan({ result: { files: { count: 1, added: 1, deleted: 0, names: [] }, checks: null, sentence: 'x' } });
    const tl = buildTimeline(p);
    expect(render(p, tl.scenes.find((s) => s.kind === 'result')!.startMs + 100, tl).strings).toContain('1 file changed');
  });

  it('draws the sentence when no files are known', () => {
    const p = plan({ result: { files: null, checks: null, sentence: 'Answered the question.' } });
    const tl = buildTimeline(p);
    expect(render(p, tl.scenes.find((s) => s.kind === 'result')!.startMs + 100, tl).strings).toContain('Answered the question.');
  });

  it('draws the end card with exactly the fixed label', () => {
    const p = plan();
    const tl = buildTimeline(p);
    const end = tl.scenes.find((s) => s.kind === 'end')!;
    expect(render(p, end.startMs + 100, tl).strings.filter((s) => s === 'Made with Puppet Master')).toHaveLength(1);
  });

  it('fades the end card to dark over the last half second', () => {
    const p = plan();
    const tl = buildTimeline(p);
    const end = tl.scenes.find((s) => s.kind === 'end')!;
    const r = render(p, end.endMs - 250, tl);
    expect(r.boxes.some((b) => b.fill === CLIP_COLORS.bg && b.w === CLIP_WIDTH && Math.abs(b.alpha - 0.5) < 1e-9)).toBe(true);
  });

  it('draws the end card for any time past the end', () => {
    const p = plan();
    const tl = buildTimeline(p);
    expect(render(p, tl.totalMs + 99_999, tl).strings).toContain('Made with Puppet Master');
  });
});

describe('second wall (GuardCounter)', () => {
  it('draws [hidden] for a secret in a terminal row and counts it', () => {
    const p = plan({ replay: terminal([{ atMs: 0, rows: [0], cursor: null }], [[{ t: `key ${SECRET}`, s: 0 }]]) });
    const r = render(p, REPLAY_AT + 10);
    expect(r.strings.some((s) => s.includes('[hidden]'))).toBe(true);
    expect(r.strings.some((s) => s.includes('sk-'))).toBe(false);
    expect(r.guard.hits).toBeGreaterThan(0);
  });

  it('draws [hidden] for a secret in an activity line', () => {
    const p = plan({ replay: { kind: 'activity', lengthMs: 4000 } as Replay, steps: [step(0, 'Run', SECRET)] });
    const r = render(p, REPLAY_AT + 10);
    expect(r.strings.some((s) => s.includes('[hidden]'))).toBe(true);
    expect(r.strings.some((s) => s.includes('sk-'))).toBe(false);
    expect(r.guard.hits).toBeGreaterThan(0);
  });

  it('draws [hidden] for a home folder in the request on the title', () => {
    const r = render(plan({ request: 'Open C:\\Users\\you\\proj\\a.rs' }), 500);
    expect(r.strings).toContain('Open [hidden]');
    expect(r.guard.hits).toBeGreaterThan(0);
  });
});

describe('every fillText string comes from the plan, the captions or a fixed label', () => {
  it('holds for every frame of a rich clip', () => {
    const steps = [
      step(100, 'Read', 'auth.ts', { paths: ['auth.ts'] }),
      step(1200, 'Run', 'npm test', { ok: false }),
      step(2500, 'Edit', 'login.ts', { writes: true }),
    ];
    const rowTable: Run[][] = [
      [{ t: '$ npm test', s: 1 }],
      [{ t: 'PASS login', s: 0 }],
    ];
    const frames: Frame[] = [
      { atMs: 0, rows: [0], cursor: [0, 2] },
      { atMs: 1000, rows: [1], cursor: null },
    ];
    const p = plan({
      replay: terminal(frames, rowTable),
      steps,
      result: { files: { count: 2, added: 9, deleted: 3, names: ['auth.ts', 'login.ts'] }, checks: 'all checks passed (2/2)', sentence: 'Fixed the login.' },
    });
    const tl = buildTimeline(p);
    const sources = [
      p.agent,
      p.request,
      p.result.sentence,
      p.result.checks ?? '',
      ...p.result.files!.names,
      ...steps.flatMap((s) => [s.name, s.summary]),
      ...rowTable.flatMap((row) => row.map((run) => run.t)),
      ...tl.captions.map((c) => c.text),
    ];
    const fixed = new Set(['Puppet Master', 'Made with Puppet Master', 'failed', '']);
    const numbers = [/^[+-]\d+$/, /^\d+ files? changed$/, /^Sped up \d+x$/];

    const seen = new Set<string>();
    for (let t = 0; t <= tl.totalMs; t += 250) {
      for (const drawn of render(p, t, tl).texts) {
        seen.add(drawn.text);
        const s = drawn.text;
        const bare = s.replace(/…$/, '');
        const ok = fixed.has(s) || numbers.some((re) => re.test(s)) || sources.some((src) => src.includes(bare));
        expect(ok, `unexpected text "${s}"`).toBe(true);
      }
    }
    expect(seen.has('Made with Puppet Master')).toBe(true);
    expect(seen.has('Sped up 40x')).toBe(true);
  });
});
