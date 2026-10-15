import { describe, expect, it } from 'vitest';
import { ITEM_HIGH, ITEM_LOW, ITEM_MEDIUM, ITEM_NO_PROMPT, ITEM_SIGNIN, ITEM_TRUST, tombstone } from '../dev/attention-fixtures';
import { buttonsFor, cardView, IDLE, type CardView, type LocalAnswer } from './approval-card';
import { COPY, formatClock } from './attention-copy';

const NOW = Date.now();
const view = (item = ITEM_LOW, local: LocalAnswer = IDLE, canAnswer = true): CardView => cardView(item, { canAnswer, local, now: NOW });
const labels = (v: CardView) => v.buttons.map((b) => b.label);

describe('the open card', () => {
  it('a low-risk prompt leads with the agent’s first choice and says Low risk', () => {
    const v = view(ITEM_LOW);
    expect(v.state).toBe('open');
    expect(v.glyph).toBe('waiting_input');
    expect(v.riskWord).toBe('Low risk');
    expect(labels(v)).toEqual(['Allow once', 'Deny']);
    expect(v.buttons.map((b) => b.role)).toEqual(['primary', 'secondary']);
    expect(v.buttons.some((b) => b.focus)).toBe(false);
    expect(v.caution).toBeNull();
    expect(v.provenance).toBe('Read from the screen');
  });

  it('a medium prompt says Check first, like the desktop card', () => {
    expect(view(ITEM_MEDIUM).riskWord).toBe('Check first');
  });

  it('a high-risk prompt leads with Deny, focuses it, and never offers the broad answer', () => {
    const v = view(ITEM_HIGH);
    expect(v.riskWord).toBe('High risk');
    expect(labels(v)).toEqual(['Deny', 'Allow once']);
    expect(v.buttons[0]).toMatchObject({ id: 'deny', role: 'primary', focus: true });
    expect(v.buttons[1]).toMatchObject({ id: 'allow_once', role: 'secondary', focus: false });
    expect(labels(v)).not.toContain('Allow all commands');
    expect(v.caution).toBe('Read this before you allow it. It may be hard to undo.');
    expect(v.reasons).toHaveLength(3);
  });

  it('shows the pane’s name, the agent and how long it has waited, never an id', () => {
    const v = view(ITEM_HIGH);
    expect(v.heading).toBe('Fix login redirect');
    expect(v.meta).toBe('Claude Code · 2 min');
    expect(JSON.stringify(v)).not.toContain('pane-claude');
    expect(JSON.stringify(v)).not.toContain(ITEM_HIGH.id);
  });

  it('a folder-trust question leads with Don’t trust', () => {
    const v = view(ITEM_TRUST);
    expect(v.title).toBe('Trust this folder?');
    expect(labels(v)).toEqual(['Don’t trust', 'Trust this folder']);
    expect(v.buttons[0]).toMatchObject({ role: 'primary', focus: true });
    // A decision about a folder, not a risk reading: no risk word or glyph, and the sentence is not a command.
    expect(v).toMatchObject({ riskWord: null, riskLevel: null, summaryIsCommand: false, details: false, caution: null });
    expect(v.summary).toContain('wants to work in puppet-master');
  });

  it('the details never hold more than six reasons', () => {
    const reasons = Array.from({ length: 9 }, (_, i) => `reason ${i}`);
    const v = view({ ...ITEM_HIGH, risk: { ...ITEM_HIGH.risk!, reasons } });
    expect(v.reasons).toEqual(reasons.slice(0, 6));
  });
});

describe('cards that cannot be answered here', () => {
  it('a sign-in screen has no buttons and says it is for the PC', () => {
    const v = view(ITEM_SIGNIN);
    expect(v.state).toBe('signin');
    expect(v.buttons).toEqual([]);
    expect(v.title).toBe('Claude Code needs you on your PC.');
    expect(v.status).toBe("It's showing a sign-in screen. Puppet Master won't answer it for you.");
    expect(v.openPane).toBe(true);
  });

  it('a prompt Puppet Master could not read offers the pane', () => {
    const v = view(ITEM_NO_PROMPT);
    expect(v.state).toBe('no_prompt');
    expect(v.title).toBe('Codex is waiting for you');
    expect(v.status).toBe("Puppet Master can't read this question. Open the pane to answer it.");
    expect(v.buttons).toEqual([]);
    expect(v.openPane).toBe(true);
  });

  it('an older PC that sends no fingerprint gets no buttons either', () => {
    const { fingerprint: _f, ...rest } = ITEM_LOW.prompt!;
    expect(view({ ...ITEM_LOW, prompt: rest }).state).toBe('no_prompt');
  });

  it('a phone that may only watch sees the card without buttons', () => {
    const v = view(ITEM_LOW, IDLE, false);
    expect(v.state).toBe('watch_only');
    expect(v.buttons).toEqual([]);
    expect(v.glyph).toBe('waiting_input');
    expect(v.status).toBe('This phone can watch only. To answer here, allow it on your PC: Settings › Phone.');
    // The risk and the command stay readable.
    expect(v.riskWord).toBe('Low risk');
    expect(v.summary).toContain('npm test');
    expect(v.details).toBe(true);
  });
});

describe('an answer on its way', () => {
  const sending = (ago: number): LocalAnswer => ({ phase: 'sending', choiceId: 'allow_once', label: 'Allow once', startedAt: NOW - ago });

  it('says Sending, then Sending to your PC, then Still sending, with the buttons off', () => {
    const early = cardView(ITEM_LOW, { canAnswer: true, local: sending(300), now: NOW });
    expect(early).toMatchObject({ state: 'sending', glyph: 'running', status: 'Sending' });
    expect(early.buttons.every((b) => b.disabled)).toBe(true);
    expect(cardView(ITEM_LOW, { canAnswer: true, local: sending(2_000), now: NOW }).status).toBe('Sending to your PC');
    expect(cardView(ITEM_LOW, { canAnswer: true, local: sending(8_500), now: NOW }).status).toBe("Still sending. Your PC hasn't confirmed yet.");
  });

  it('after twenty seconds it says it did not hear back and offers Try again and the pane', () => {
    const v = cardView(ITEM_LOW, { canAnswer: true, local: sending(21_000), now: NOW });
    expect(v).toMatchObject({ state: 'lost', glyph: 'error', status: COPY.lost, retry: true, openPane: true });
    expect(v.buttons).toEqual([]);
  });

  it('a failed send says why: offline (it never left), lost, or refused', () => {
    const failed = (why: 'offline' | 'lost' | 'server'): LocalAnswer => ({ phase: 'failed', why, choiceId: 'allow_once', label: 'Allow once' });
    expect(cardView(ITEM_LOW, { canAnswer: true, local: failed('offline'), now: NOW })).toMatchObject({
      state: 'offline',
      glyph: 'waiting_input',
      status: COPY.offline,
      retry: true,
    });
    expect(cardView(ITEM_LOW, { canAnswer: true, local: failed('lost'), now: NOW })).toMatchObject({ state: 'lost', glyph: 'error', retry: true });
    expect(cardView(ITEM_LOW, { canAnswer: true, local: failed('server'), now: NOW })).toMatchObject({
      state: 'failed',
      status: COPY.failed('Codex'),
      retry: true,
    });
  });
});

describe('what the PC said back', () => {
  it('answered: the receipt line, no buttons', () => {
    const at = new Date(2026, 9, 15, 9, 41).getTime();
    const v = cardView(ITEM_LOW, { canAnswer: true, local: { phase: 'answered', label: 'Allowed once', at }, now: NOW });
    expect(v).toMatchObject({ state: 'answered', glyph: 'exited', settled: true, status: 'Allowed once · 09:41 · from this phone' });
    expect(v.buttons).toEqual([]);
    expect(formatClock(at)).toBe('09:41');
  });

  it('already answered: on the PC, by you, or from another phone by name', () => {
    const already = (by: 'pc' | 'you' | 'phone', deviceName?: string): CardView =>
      cardView(ITEM_LOW, { canAnswer: true, local: { phase: 'already', by, deviceName, label: 'Denied' }, now: NOW });
    expect(already('pc').status).toBe('Already answered on your PC: Denied.');
    expect(already('you').status).toBe('Already answered by you: Denied.');
    expect(already('phone', 'Anna’s iPhone').status).toBe('Already answered from Anna’s iPhone: Denied.');
    expect(already('phone').status).toBe('Already answered from another phone: Denied.');
    expect(already('pc').glyph).toBe('exited');
  });

  it('changed: the stale copy, a plus glyph and no buttons', () => {
    const v = cardView(ITEM_LOW, { canAnswer: true, local: { phase: 'changed' }, now: NOW });
    expect(v).toMatchObject({ state: 'changed', glyph: 'waiting_input', status: 'This question changed. Look again.', lookAgain: true });
    expect(v.buttons).toEqual([]);
  });

  it('gone: says the agent moved on and offers the pane', () => {
    const v = cardView(ITEM_HIGH, { canAnswer: true, local: { phase: 'gone' }, now: NOW });
    expect(v).toMatchObject({ state: 'gone', glyph: 'idle', status: 'This question has gone. Claude Code moved on.', openPane: true });
  });
});

describe('what the list says wins over the request', () => {
  const sending: LocalAnswer = { phase: 'sending', choiceId: 'allow_once', label: 'Allow once', startedAt: NOW - 1_000 };

  it('a resolved item shows its end state at once, whatever the HTTP call is doing', () => {
    const resolved = tombstone(ITEM_LOW, { state: 'answered', by: 'phone', choice_label: 'Allowed once', at_ms: new Date(2026, 9, 15, 9, 41).getTime() });
    const v = cardView(resolved, { canAnswer: true, local: sending, now: NOW });
    expect(v.state).toBe('answered');
    expect(v.status).toBe('Allowed once · 09:41 · from this phone');
  });

  it('answered at the PC first, or from another phone', () => {
    const pc = tombstone(ITEM_LOW, { state: 'answered', by: 'desktop', choice_label: 'Allowed once' });
    expect(view(pc).status).toBe('Already answered on your PC: Allowed once.');
    const other = tombstone(ITEM_LOW, { state: 'answered', by: 'phone', choice_label: 'Denied', device_name: 'Ben’s Pixel' });
    expect(view(other).status).toBe('Already answered from Ben’s Pixel: Denied.');
  });

  it('gone, expired and moved on', () => {
    expect(view(tombstone(ITEM_LOW, { state: 'gone', reason: 'moved_on' })).status).toBe('This question has gone. Codex moved on.');
    expect(view(tombstone(ITEM_LOW, { state: 'gone', reason: 'pane_closed' })).state).toBe('gone');
    const expired = view(tombstone(ITEM_LOW, { state: 'expired', at_ms: new Date(2026, 9, 15, 11, 41).getTime() }));
    expect(expired).toMatchObject({ state: 'expired', glyph: 'idle', status: 'Codex stopped waiting at 11:41.' });
  });

  it('a tombstone never has buttons', () => {
    for (const state of ['answered', 'gone', 'expired', 'withdrawn']) {
      expect(view(tombstone(ITEM_HIGH, { state })).buttons).toEqual([]);
    }
  });
});

describe('buttons', () => {
  it('never offers a broad answer, even when the layout forgot to tuck it away', () => {
    const item = { ...ITEM_HIGH, prompt: { ...ITEM_HIGH.prompt!, layout: { primary: 'deny', focus: 'deny', more: [] } } };
    expect(buttonsFor(item, false).map((b) => b.id)).toEqual(['deny', 'allow_once']);
  });

  it('keeps every offered choice when the PC sent no layout (an older PC)', () => {
    const { layout: _l, ...prompt } = ITEM_LOW.prompt!;
    const ids = buttonsFor({ ...ITEM_LOW, prompt }, false).map((b) => b.id);
    expect(ids).toEqual(['allow_once', 'deny']);
  });
});
