// @vitest-environment node
// The first run's rules: which choices are offered, what the scripted first message says in each persona, and when a
// bot counts as new. The screen is in ThreadView.firstrun.test.tsx.
import { describe, expect, it } from 'vitest';
import { FIRST_RUN_CHOICES, firstRunScript, isNewBot, visibleChoices } from './potato-firstrun';
import type { PotatoMessage } from './potato';

const message = (text: string): PotatoMessage => ({ id: 'm1', at: 1, from: { kind: 'you' }, kind: 'text', text });

describe('isNewBot', () => {
  it('is new only while the bot has no last-message time and no rows', () => {
    expect(isNewBot({ lastAt: null }, [])).toBe(true);
    expect(isNewBot({ lastAt: 1_700_000_000_000 }, [])).toBe(false);
    expect(isNewBot({ lastAt: null }, [message('hi')])).toBe(false);
  });
});

describe('visibleChoices', () => {
  it('offers four choices, and leaves "Use the computer" out while the sandbox is off', () => {
    expect(visibleChoices(true).map((c) => c.label)).toEqual(['Just chat', 'Help with a project', 'Use the computer', 'What can you do?']);
    expect(visibleChoices(false).map((c) => c.label)).toEqual(['Just chat', 'Help with a project', 'What can you do?']);
  });

  it('sends the right first message for each button', () => {
    const text = Object.fromEntries(FIRST_RUN_CHOICES.map((c) => [c.label, c.text]));
    expect(text).toEqual({
      'Just chat': "Let's just chat.",
      'Help with a project': 'I want help with a project.',
      'Use the computer': 'Use your computer to help me.',
      'What can you do?': 'What can you do?',
    });
  });
});

describe('firstRunScript', () => {
  it('speaks in each persona, and names the bot', () => {
    const fish = firstRunScript('fish', 'Chief', true);
    const potato = firstRunScript('potato', 'Chief', true);
    expect(fish.greeting).toContain("I'm Chief");
    expect(potato.greeting).toContain("I'm Chief");
    expect(fish.greeting).not.toBe(potato.greeting);
    expect(fish.question).not.toBe(potato.question);
  });

  it('says what it can do: chat, a helper that asks first, and the computer when the sandbox is on', () => {
    const on = firstRunScript('fish', 'Fish', true);
    expect(on.points.join(' ')).toMatch(/Chat with me/);
    expect(on.points.join(' ')).toMatch(/helper agent to your project folder\. It asks you first/);
    expect(on.points.join(' ')).toMatch(/own computer \(a sandbox\)/);
  });

  it('leaves the computer line out while the sandbox is off', () => {
    const off = firstRunScript('potato', 'Spud', false);
    expect(off.points).toHaveLength(2);
    expect(off.points.join(' ')).not.toMatch(/computer/);
  });
});
