import { describe, expect, it } from 'vitest';
import { abilitiesFrom, ALL_ABILITIES } from './phone-abilities';

describe('abilitiesFrom (GET /pair/me)', () => {
  it('a phone with type keeps its input box and shows no note', () => {
    expect(abilitiesFrom({ local: false, caps: ['watch', 'answer', 'type', 'control'] })).toEqual({
      canWatch: true,
      canType: true,
      canAnswer: true,
      canControl: true,
      typingNote: null,
    });
  });

  it('a phone without type gets the plain line', () => {
    expect(abilitiesFrom({ local: false, caps: ['watch', 'answer'] })).toEqual({
      canWatch: true,
      canType: false,
      canAnswer: true,
      canControl: false,
      typingNote: 'This phone can watch and answer, not type.',
    });
  });

  it('a watch-only phone does not claim it can answer', () => {
    expect(abilitiesFrom({ local: false, caps: ['watch'] }).typingNote).toBe('This phone can watch, not type.');
  });

  it('a phone without watch cannot read chats', () => {
    expect(abilitiesFrom({ local: false, caps: ['answer'] }).canWatch).toBe(false);
    expect(abilitiesFrom({ local: false, caps: ['watch'] }).canWatch).toBe(true);
    expect(ALL_ABILITIES.canWatch).toBe(true);
  });

  it('an unknown answer (old PC, no route, a bad body) hides nothing', () => {
    expect(abilitiesFrom(null)).toBe(ALL_ABILITIES);
    expect(abilitiesFrom(undefined)).toBe(ALL_ABILITIES);
    expect(abilitiesFrom({ local: false } as never)).toBe(ALL_ABILITIES);
  });
});
