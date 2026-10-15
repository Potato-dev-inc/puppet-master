// @vitest-environment jsdom
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ITEM_HIGH, ITEM_LOW, tombstone } from '../../dev/attention-fixtures';
import { AttentionAnswerError, type AttentionAnswerBody, type AttentionAnswerOutcome, type AttentionItem } from '../../lib/attention';
import { ApprovalCard } from './ApprovalCard';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  vi.useFakeTimers();
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.useRealTimers();
});

type Call = { id: string; body: AttentionAnswerBody; signal?: AbortSignal };

function bridgeReturning(reply: (call: Call, n: number) => Promise<AttentionAnswerOutcome>) {
  const calls: Call[] = [];
  const answerAttention = vi.fn((id: string, body: AttentionAnswerBody, signal?: AbortSignal) => {
    const call = { id, body, signal };
    calls.push(call);
    return reply(call, calls.length);
  });
  return { bridge: { answerAttention }, calls };
}

const answered: AttentionAnswerOutcome = { outcome: 'answered', choice_label: 'Allowed once', at_ms: new Date(2026, 9, 15, 9, 41).getTime() };

async function render(item: AttentionItem, bridge: { answerAttention: unknown } | null, canAnswer = true, autoFocus = false) {
  await act(async () => {
    root.render(<ApprovalCard item={item} bridge={bridge as never} canAnswer={canAnswer} autoFocus={autoFocus} onOpenPane={() => undefined} />);
  });
}

const button = (text: string) => Array.from(host.querySelectorAll('button')).find((b) => b.textContent === text) as HTMLButtonElement | undefined;
const state = () => host.querySelector('[data-card-state]')?.getAttribute('data-card-state');
const status = () => host.querySelector('.ph-appr-status')?.textContent ?? '';
const click = async (text: string) => {
  await act(async () => {
    button(text)!.click();
  });
};

describe('the answer', () => {
  it('sends the choice with exactly what the card showed and a fresh answer key', async () => {
    const { bridge, calls } = bridgeReturning(async () => answered);
    await render(ITEM_LOW, bridge);
    await click('Allow once');
    expect(calls).toHaveLength(1);
    expect(calls[0].id).toBe(ITEM_LOW.id);
    expect(calls[0].body).toMatchObject({
      choice: 'allow_once',
      prompt_id: 'pid-low',
      occurrence: 2,
      fingerprint: 'f3c1a9b200d4e7a1',
    });
    expect(calls[0].body.answer_key).toMatch(/^[\x21-\x7e]{8,80}$/);
    expect(state()).toBe('answered');
    expect(status()).toBe('Allowed once · 09:41 · from this phone');
    expect(button('Allow once')).toBeUndefined();
  });

  it('a second tap while the first is on its way does not send a second answer', async () => {
    const { bridge, calls } = bridgeReturning(() => new Promise(() => {}));
    await render(ITEM_LOW, bridge);
    await click('Allow once');
    // The buttons are off while sending.
    expect(button('Allow once')!.disabled).toBe(true);
    await act(async () => button('Deny')!.click());
    expect(calls).toHaveLength(1);
  });

  it('says what the PC said: changed, gone, answered on the PC', async () => {
    for (const [reply, text, expected] of [
      [{ outcome: 'changed' }, 'changed', 'This question changed. Look again.'],
      [{ outcome: 'gone' }, 'gone', 'This question has gone. Codex moved on.'],
      [{ outcome: 'already_answered', by: 'pc', choice_label: 'Allowed once' }, 'already', 'Already answered on your PC: Allowed once.'],
    ] as const) {
      const { bridge } = bridgeReturning(async () => reply as AttentionAnswerOutcome);
      await render(ITEM_LOW, bridge);
      await click('Allow once');
      expect(state(), text).toBe(text);
      expect(status()).toBe(expected);
      expect(host.querySelectorAll('.ph-appr-actions button[data-choice]')).toHaveLength(0);
      act(() => root.unmount());
      root = createRoot(host);
    }
  });

  it('a refusal or a key that could not be written says nothing was typed and offers Try again', async () => {
    const { bridge } = bridgeReturning(async () => ({ outcome: 'failed', code: 'PROMPT_REPLY_FAILED' }));
    await render(ITEM_LOW, bridge);
    await click('Allow once');
    expect(state()).toBe('failed');
    expect(status()).toBe("Couldn't send your answer to Codex. Nothing was typed.");
    expect(button('Try again')).toBeDefined();
    expect(button('Open pane')).toBeDefined();
  });
});

describe('an answer that is stuck', () => {
  it('walks Sending, Sending to your PC, Still sending, then did not hear back with Try again', async () => {
    const { bridge, calls } = bridgeReturning((call) => new Promise((_, reject) => call.signal?.addEventListener('abort', () => reject(new DOMException('aborted', 'AbortError')))));
    await render(ITEM_LOW, bridge);
    await click('Allow once');
    expect(status()).toBe('Sending');
    await act(async () => void vi.advanceTimersByTime(2_000));
    expect(status()).toBe('Sending to your PC');
    await act(async () => void vi.advanceTimersByTime(7_000));
    expect(status()).toBe("Still sending. Your PC hasn't confirmed yet.");
    expect(button('Allow once')!.disabled).toBe(true);
    await act(async () => void vi.advanceTimersByTime(11_500));
    expect(state()).toBe('lost');
    expect(status()).toBe("Didn't hear back from your PC. Your answer may not have arrived.");
    expect(calls[0].signal?.aborted).toBe(true);
    expect(button('Try again')).toBeDefined();
  });

  it('Try again sends the SAME answer key, so the PC can recognise the first tap', async () => {
    const { bridge, calls } = bridgeReturning(async (_call, n) => {
      if (n === 1) throw new TypeError('Failed to fetch');
      return { outcome: 'already_answered', by: 'you', choice_label: 'Allowed once' };
    });
    await render(ITEM_LOW, bridge);
    await click('Allow once');
    expect(state()).toBe('offline');
    expect(status()).toBe("Can't reach your PC. Your answer wasn't sent.");
    await click('Try again');
    expect(calls).toHaveLength(2);
    expect(calls[1].body.answer_key).toBe(calls[0].body.answer_key);
    expect(calls[1].body.choice).toBe('allow_once');
    expect(status()).toBe('Already answered by you: Allowed once.');
  });

  it('a new tap after a settled card is a new answer with a new key', async () => {
    const { bridge, calls } = bridgeReturning(async (_call, n) => (n === 1 ? ({ outcome: 'failed' } as AttentionAnswerOutcome) : answered));
    await render(ITEM_LOW, bridge);
    await click('Allow once');
    await click('Try again');
    expect(calls[1].body.answer_key).toBe(calls[0].body.answer_key);
    expect(state()).toBe('answered');
  });

  it('what the list says wins: a resolved frame replaces the card while the call is still on its way', async () => {
    const { bridge } = bridgeReturning(() => new Promise(() => {}));
    await render(ITEM_LOW, bridge);
    await click('Allow once');
    expect(state()).toBe('sending');
    const frame = tombstone(ITEM_LOW, { state: 'answered', by: 'phone', choice_label: 'Allowed once', at_ms: new Date(2026, 9, 15, 9, 41).getTime() });
    await render(frame, bridge);
    expect(state()).toBe('answered');
    expect(status()).toBe('Allowed once · 09:41 · from this phone');
  });

  it('a refusal from the PC (not allowed) is not retried silently', async () => {
    const { bridge, calls } = bridgeReturning(async () => {
      throw new AttentionAnswerError(403, { code: 'DEVICE_CAP_REQUIRED', cap: 'answer' });
    });
    await render(ITEM_LOW, bridge);
    await click('Allow once');
    expect(calls).toHaveLength(1);
    expect(state()).toBe('failed');
  });
});

describe('who sees buttons', () => {
  it('a phone that may only watch sees the card, the risk and the command, and no buttons', async () => {
    const { bridge, calls } = bridgeReturning(async () => answered);
    await render(ITEM_LOW, bridge, false);
    expect(state()).toBe('watch_only');
    expect(host.querySelectorAll('button[data-choice]')).toHaveLength(0);
    expect(host.textContent).toContain('This phone can watch only');
    expect(host.textContent).toContain('Low risk');
    expect(host.textContent).toContain('npm test');
    expect(button('Show details')).toBeDefined();
    expect(calls).toHaveLength(0);
  });

  it('high risk puts Deny first and focuses it, never Allow once', async () => {
    const { bridge } = bridgeReturning(async () => answered);
    await render(ITEM_HIGH, bridge, true, true);
    const order = Array.from(host.querySelectorAll('button[data-choice]')).map((b) => b.textContent);
    expect(order).toEqual(['Deny', 'Allow once']);
    expect((document.activeElement as HTMLElement | null)?.textContent).toBe('Deny');
    expect(host.querySelector('.ph-appr-actions .ph-btn--primary')?.textContent).toBe('Deny');
    expect(host.textContent).toContain('Read this before you allow it. It may be hard to undo.');
  });

  it('shows no raw id anywhere', async () => {
    const { bridge } = bridgeReturning(async () => answered);
    await render(ITEM_HIGH, bridge);
    expect(host.textContent).not.toMatch(/pane-claude|h1ghR1sk|pid-high|c0de5afe/);
  });

  it('Show details reveals the full command and the reasons', async () => {
    const { bridge } = bridgeReturning(async () => answered);
    await render(ITEM_HIGH, bridge);
    expect(host.querySelector('.ph-appr-reasons')).toBeNull();
    await click('Show details');
    expect(host.querySelectorAll('.ph-appr-reasons li')).toHaveLength(3);
    expect(button('Hide details')).toBeDefined();
  });
});

describe('no voice', () => {
  it('the card is silent: nothing speaks, and nothing in its code can', async () => {
    const speak = vi.fn();
    (window as unknown as { speechSynthesis: unknown }).speechSynthesis = { speak };
    const { bridge } = bridgeReturning(async () => answered);
    await render(ITEM_HIGH, bridge);
    await click('Deny');
    expect(speak).not.toHaveBeenCalled();
    for (const file of ['ApprovalCard.tsx', 'DockedCard.tsx', 'NeedsYouList.tsx']) {
      const text = readFileSync(join(process.cwd(), 'src/components/mobile', file), 'utf8');
      expect(text, file).not.toMatch(/speechSynthesis|SpeechRecognition|webkitSpeech/);
    }
  });
});
