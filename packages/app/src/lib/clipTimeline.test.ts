import { describe, expect, it } from 'vitest';
import type { ClipPlan, Replay, Step } from './clipTypes';
import { buildTimeline, captionFor, CAPTION_MIN_MS, CLIP_MAX_MS, type Caption, type Timeline } from './clipTimeline';

function step(atMs: number, name: string, extra: Partial<Step> = {}): Step {
  return { atMs, name, summary: `${name} target`, paths: [], writes: false, helper: false, ok: true, ...extra };
}

function plan(replay: Replay, steps: Step[] = [], extra: Partial<ClipPlan> = {}): ClipPlan {
  return {
    token: 'tok',
    agent: 'Test Agent',
    request: 'Fix the login page',
    turnMs: 60_000,
    outcome: 'completed',
    replay,
    steps,
    result: { files: null, checks: null, sentence: 'Done.' },
    notes: [],
    hidden: 0,
    ...extra,
  };
}

const terminal = (lengthMs: number, speedX = 40): Replay => ({
  kind: 'terminal',
  cols: 80,
  rows: 24,
  rowFrom: 0,
  rowTo: 23,
  speedX,
  lengthMs,
  rowTable: [],
  frames: [],
});

const activity = (lengthMs: number): Replay => ({ kind: 'activity', lengthMs });

const words = (text: string) => text.split(/\s+/).filter((word) => word !== '').length;
const plainCaptions = (timeline: Timeline) => timeline.captions.filter((caption) => caption.role === 'caption');

/** The privacy-free checks every timeline must pass. */
function expectSound(timeline: Timeline) {
  expect(timeline.totalMs).toBeLessThanOrEqual(CLIP_MAX_MS);
  expect(timeline.scenes[0]?.startMs).toBe(0);
  for (let i = 1; i < timeline.scenes.length; i++) {
    expect(timeline.scenes[i]!.startMs).toBe(timeline.scenes[i - 1]!.endMs);
  }
  expect(timeline.scenes[timeline.scenes.length - 1]!.endMs).toBe(timeline.totalMs);

  const captions = plainCaptions(timeline);
  expect(captions.length).toBeLessThanOrEqual(4);
  const sorted = [...captions].sort((a, b) => a.startMs - b.startMs);
  for (const caption of sorted) {
    expect(caption.endMs - caption.startMs).toBeGreaterThanOrEqual(CAPTION_MIN_MS);
    expect(words(caption.text)).toBeLessThanOrEqual(12);
  }
  for (let i = 1; i < sorted.length; i++) {
    expect(sorted[i]!.startMs).toBeGreaterThanOrEqual(sorted[i - 1]!.endMs);
  }
}

describe('buildTimeline scenes', () => {
  it('lays out title, replay, result and end card for a terminal plan', () => {
    const timeline = buildTimeline(plan(terminal(12_000), [step(0, 'Read')]));
    expect(timeline.scenes.map((scene) => scene.kind)).toEqual(['title', 'replay', 'result', 'end']);
    expect(timeline.scenes[1]).toMatchObject({ kind: 'replay', startMs: 3_000, endMs: 15_000, mode: 'terminal' });
    expect(timeline.totalMs).toBe(3_000 + 12_000 + 5_000 + 2_500);
    expectSound(timeline);
  });

  it('lays out the same scenes for an activity plan', () => {
    const timeline = buildTimeline(plan(activity(8_000), [step(0, 'Read')]));
    expect(timeline.scenes.map((scene) => scene.kind)).toEqual(['title', 'replay', 'result', 'end']);
    expect(timeline.scenes[1]).toMatchObject({ mode: 'activity' });
    expect(timeline.totalMs).toBe(3_000 + 8_000 + 5_000 + 2_500);
    expectSound(timeline);
  });

  it('drops the replay scene for a none plan and makes a 10.5 s clip', () => {
    const timeline = buildTimeline(plan({ kind: 'none' }, [step(0, 'Read')]));
    expect(timeline.scenes.map((scene) => scene.kind)).toEqual(['title', 'result', 'end']);
    expect(timeline.totalMs).toBe(10_500);
    expect(timeline.captions).toEqual([]);
    expectSound(timeline);
  });

  it('keeps the agent label on the title and the end card', () => {
    const timeline = buildTimeline(plan(activity(4_000)));
    expect(timeline.scenes[0]).toMatchObject({ kind: 'title', agent: 'Test Agent', request: 'Fix the login page' });
    expect(timeline.scenes[timeline.scenes.length - 1]).toMatchObject({ kind: 'end', agent: 'Test Agent' });
  });

  it('keeps the replay between 4 and 18 seconds', () => {
    expect(buildTimeline(plan(terminal(0))).totalMs).toBe(3_000 + 4_000 + 5_000 + 2_500);
    expect(buildTimeline(plan(activity(Number.NaN))).totalMs).toBe(3_000 + 4_000 + 5_000 + 2_500);
    expect(buildTimeline(plan(terminal(10 * 3600 * 1000))).totalMs).toBe(3_000 + 18_000 + 5_000 + 2_500);
    expect(buildTimeline(plan(activity(18_000))).totalMs).toBe(28_500);
  });
});

describe('buildTimeline hard cap', () => {
  const lengths = [0, 1, 3_999, 4_000, 12_345, 18_000, 18_001, 36_000_000, Number.NaN, -5, Number.POSITIVE_INFINITY];
  const stepCounts = [0, 1, 1_000];

  it('never runs past 30 seconds, for every replay length and step count', () => {
    for (const length of lengths) {
      for (const count of stepCounts) {
        const steps = Array.from({ length: count }, (_, i) => step(i * 900, 'Read', { paths: [`f${i}.ts`] }));
        for (const replay of [terminal(length), activity(length)]) {
          const timeline = buildTimeline(plan(replay, steps));
          expect(timeline.totalMs).toBeLessThanOrEqual(CLIP_MAX_MS);
          expectSound(timeline);
        }
      }
    }
  });

  it('is at most 28.5 seconds with a long replay', () => {
    expect(buildTimeline(plan(terminal(10 ** 9))).totalMs).toBe(28_500);
  });
});

describe('buildTimeline captions', () => {
  it('shows a badge with the speed over the replay of a terminal plan', () => {
    const timeline = buildTimeline(plan(terminal(12_000, 40)));
    const badge = timeline.captions.find((caption) => caption.role === 'badge');
    expect(badge).toEqual({ startMs: 3_000, endMs: 15_000, text: 'Sped up 40x', role: 'badge' });
  });

  it('shows no badge at real speed or for an activity plan', () => {
    expect(buildTimeline(plan(terminal(12_000, 1))).captions.some((caption) => caption.role === 'badge')).toBe(false);
    expect(buildTimeline(plan(activity(12_000), [step(0, 'Read')])).captions.some((caption) => caption.role === 'badge')).toBe(false);
  });

  it('puts each step group in its own part of the replay, with its own time', () => {
    const steps = [step(0, 'Read', { paths: ['a.ts'] }), step(9_000, 'Bash')];
    const timeline = buildTimeline(plan(terminal(12_000), steps));
    const captions = plainCaptions(timeline);
    expect(captions).toEqual([
      { startMs: 3_000, endMs: 6_000, text: 'Read 1 file', role: 'caption' },
      { startMs: 12_000, endMs: 15_000, text: 'Ran 1 command', role: 'caption' },
    ]);
  });

  it('merges steps into at most 4 captions for a long activity replay', () => {
    const steps = Array.from({ length: 30 }, (_, i) => step(i * 900, i % 2 === 0 ? 'Read' : 'Bash', { paths: [`f${i}.ts`] }));
    const timeline = buildTimeline(plan(activity(18_000), steps));
    expect(plainCaptions(timeline).length).toBeLessThanOrEqual(4);
    expectSound(timeline);
  });

  it('has no captions when there are no steps', () => {
    expect(buildTimeline(plan(activity(9_000))).captions).toEqual([]);
  });

  it('keeps every caption inside the replay scene', () => {
    const steps = [step(-50, 'Read'), step(99_999, 'Bash')];
    const timeline = buildTimeline(plan(terminal(6_000), steps));
    const replay = timeline.scenes.find((scene) => scene.kind === 'replay')!;
    for (const caption of timeline.captions) {
      expect(caption.startMs).toBeGreaterThanOrEqual(replay.startMs);
      expect(caption.endMs).toBeLessThanOrEqual(replay.endMs);
    }
  });
});

describe('captionFor', () => {
  it('uses the activity wording', () => {
    const steps = [
      step(0, 'Read', { paths: ['a.ts'] }),
      step(100, 'Read', { paths: ['b.ts'] }),
      step(200, 'Read', { paths: ['c.ts'] }),
      step(300, 'Read', { paths: ['d.ts'] }),
      step(400, 'Read', { paths: ['e.ts'] }),
      step(500, 'Read', { paths: ['f.ts'] }),
      step(600, 'Bash'),
      step(700, 'Bash'),
    ];
    expect(captionFor(steps)).toBe('Read 6 files · Ran 2 commands');
  });

  it('adds a failure count when a command failed', () => {
    expect(captionFor([step(0, 'Bash', { ok: false })])).toBe('Ran 1 command · 1 failed');
    expect(captionFor([step(0, 'Bash', { ok: true }), step(10, 'Bash', { ok: false }), step(20, 'Bash', { ok: false })])).toBe(
      'Ran 3 commands · 2 failed',
    );
  });

  it('does not count a result that is not known as a failure', () => {
    expect(captionFor([step(0, 'Bash', { ok: null })])).toBe('Ran 1 command');
  });

  it('is empty for no steps', () => {
    expect(captionFor([])).toBe('');
  });

  it('keeps every caption at 12 words or fewer, and keeps a failure', () => {
    const names = ['Read', 'Grep', 'Edit', 'Write', 'Bash', 'WebFetch', 'Task', 'TodoWrite'];
    const steps = names.map((name, i) => step(i * 100, name, { paths: [`p${i}.ts`], writes: name === 'Edit', helper: name === 'Task', ok: i === 4 ? false : true }));
    const text = captionFor(steps);
    expect(words(text)).toBeLessThanOrEqual(12);
    expect(text.endsWith('1 failed')).toBe(true);
  });

  it('says Used N tools for a tool it does not know', () => {
    expect(captionFor([step(0, 'TodoWrite'), step(10, 'Mystery')])).toBe('Used 2 tools');
  });
});

describe('buildTimeline request override', () => {
  it('replaces only the request on the title', () => {
    const base = buildTimeline(plan(terminal(12_000, 40), [step(0, 'Read', { paths: ['a.ts'] })]));
    const over = buildTimeline(plan(terminal(12_000, 40), [step(0, 'Read', { paths: ['a.ts'] })]), 'Add a dark mode');
    expect(over.scenes[0]).toEqual({ ...base.scenes[0], request: 'Add a dark mode' });
    expect(over.scenes.slice(1)).toEqual(base.scenes.slice(1));
    expect(over.captions).toEqual(base.captions);
    expect(over.totalMs).toBe(base.totalMs);
  });

  it('keeps the plan request when the override is empty', () => {
    const timeline = buildTimeline(plan(activity(4_000)), '   ');
    expect(timeline.scenes[0]).toMatchObject({ request: 'Fix the login page' });
  });

  it('keeps one line and at most 140 characters', () => {
    const timeline = buildTimeline(plan(activity(4_000)), `first line\n\n${'x'.repeat(300)}`);
    const request = (timeline.scenes[0] as { request: string }).request;
    expect(request.startsWith('first line x')).toBe(true);
    expect(Array.from(request).length).toBe(140);
    expect(request.includes('\n')).toBe(false);
  });
});

describe('buildTimeline types', () => {
  it('returns caption objects with a role', () => {
    const timeline = buildTimeline(plan(terminal(6_000), [step(0, 'Read')]));
    const caption: Caption = timeline.captions[0]!;
    expect(['caption', 'badge']).toContain(caption.role);
  });
});
