import { describe, expect, it } from 'vitest';
import {
  buildWhen,
  checkInLabel,
  clockValue,
  companionSlug,
  countChars,
  formatCount,
  homeProblemTitle,
  isBroadFolderError,
  isCompanionPaneId,
  isValidName,
  movedNote,
  movesWhenFree,
  newCheckInId,
  nextClockMs,
  ownFolderIn,
  parseClock,
  plainError,
  stateLineOf,
  untilText,
} from './model';

describe('companion panes', () => {
  it('a companion pane is companion-<id>; other panes are not', () => {
    expect(isCompanionPaneId('companion-mochi')).toBe(true);
    expect(isCompanionPaneId('p1')).toBe(false);
    expect(isCompanionPaneId('my-companion-note')).toBe(false);
  });
});

describe('names', () => {
  it('a name is 1 to 24 characters with no control characters', () => {
    expect(isValidName('Mochi')).toBe(true);
    expect(isValidName('   ')).toBe(false);
    expect(isValidName('')).toBe(false);
    expect(isValidName('x'.repeat(24))).toBe(true);
    expect(isValidName('x'.repeat(25))).toBe(false);
    expect(isValidName('Mo\tchi')).toBe(false);
  });
});

describe('character counts', () => {
  it('counts Unicode characters, not UTF-16 units', () => {
    expect(countChars('abc')).toBe(3);
    expect(countChars('a😀')).toBe(2);
    expect(countChars('')).toBe(0);
  });

  it('formats with thousands separators', () => {
    expect(formatCount(1204)).toBe('1,204');
    expect(formatCount(2200)).toBe('2,200');
  });
});

describe('state line', () => {
  it('uses the backend sentence, else the plain word', () => {
    expect(stateLineOf({ state: 'idle', stateLine: 'Here. Ask me anything.' }, 'Here')).toBe('Here. Ask me anything.');
    expect(stateLineOf({ state: 'idle', stateLine: '' }, 'Here')).toBe('Here');
  });
});

describe('check-in schedules', () => {
  const at = { h: 8, m: 0 };
  const fields = { at, day: 2, hours: 3, atMs: 1_700_000_000_000 };

  it('builds each preset as the backend reads it', () => {
    expect(buildWhen('daily', fields)).toEqual({ kind: 'daily', at });
    expect(buildWhen('weekdays', fields)).toEqual({ kind: 'weekdays', at });
    expect(buildWhen('weekly', fields)).toEqual({ kind: 'weekly', day: 2, at });
    expect(buildWhen('every', fields)).toEqual({ kind: 'every', minutes: 180 });
    expect(buildWhen('once', fields)).toEqual({ kind: 'once', atMs: 1_700_000_000_000 });
  });

  it('every N hours is never less than one hour', () => {
    expect(buildWhen('every', { ...fields, hours: 0 })).toEqual({ kind: 'every', minutes: 60 });
  });

  it('reads a time from a time input, and refuses anything else', () => {
    expect(parseClock('08:05')).toEqual({ h: 8, m: 5 });
    expect(parseClock('23:59')).toEqual({ h: 23, m: 59 });
    expect(parseClock('24:00')).toBeNull();
    expect(parseClock('8am')).toBeNull();
  });

  it('labels a check-in by its first 40 characters and makes an 8-hex id', () => {
    expect(checkInLabel('  Morning brief  ')).toBe('Morning brief');
    expect(checkInLabel('a'.repeat(60))).toHaveLength(40);
    expect(newCheckInId(() => 0.5)).toMatch(/^[0-9a-f]{8}$/);
  });
});

describe('moving to another agent', () => {
  const now = new Date(2026, 9, 28, 10, 0, 0).getTime();

  it('the next moment the clock shows a time: later today, or tomorrow when it has passed', () => {
    expect(nextClockMs({ h: 17, m: 0 }, now)).toBe(new Date(2026, 9, 28, 17, 0, 0).getTime());
    expect(nextClockMs({ h: 9, m: 59 }, now)).toBe(new Date(2026, 9, 29, 9, 59, 0).getTime());
    expect(nextClockMs({ h: 10, m: 0 }, now)).toBe(new Date(2026, 9, 29, 10, 0, 0).getTime());
  });

  it('a time input value from a moment', () => {
    expect(clockValue(new Date(2026, 9, 28, 7, 5, 0).getTime())).toBe('07:05');
  });

  it('says a time as a clock today and with the weekday on another day', () => {
    expect(untilText(new Date(2026, 9, 28, 17, 0, 0).getTime(), now)).toBe('17:00');
    expect(untilText(new Date(2026, 9, 29, 9, 30, 0).getTime(), now)).toBe('Thu 09:30');
  });

  it('the waiting sentence names the companion', () => {
    expect(movesWhenFree('Mochi')).toBe('Moves when Mochi is free');
  });

  it('says where a moved companion came from, and when it goes back', () => {
    expect(movedNote({ previousAgentLabel: null, untilMs: null }, now)).toBeNull();
    expect(movedNote({ previousAgentLabel: 'Claude Code', untilMs: null }, now)).toBe('Moved from Claude Code.');
    expect(movedNote({ previousAgentLabel: 'Claude Code', untilMs: new Date(2026, 9, 28, 17, 0, 0).getTime() }, now)).toBe(
      'Moved from Claude Code. Goes back at 17:00.',
    );
  });
});

describe('a home folder that is too broad', () => {
  it('the card title says where the home is and what that does', () => {
    expect(homeProblemTitle('Pipi', 'your Desktop')).toBe("Pipi's home is your Desktop, so other agents there think they are Pipi.");
    expect(homeProblemTitle('Pipi', 'a folder with other projects in it')).toBe(
      "Pipi's home is a folder with other projects in it, so other agents there think they are Pipi.",
    );
  });

  it('the code in front of an error is taken off, other text is not', () => {
    expect(plainError('COMPANION_FOLDER_BROAD: Pick a folder of its own.')).toBe('Pick a folder of its own.');
    expect(plainError('Could not move it.')).toBe('Could not move it.');
    expect(isBroadFolderError('COMPANION_FOLDER_BROAD: Pick a folder of its own.')).toBe(true);
    expect(isBroadFolderError('COMPANION_LIMIT: no')).toBe(false);
  });

  it('the folder of its own is the picked folder plus the id the backend makes of the name', () => {
    expect(companionSlug('Pipi')).toBe('pipi');
    expect(companionSlug('  Mo Chi!  ')).toBe('mo-chi');
    expect(companionSlug('!!!')).toBe('companion');
    expect(companionSlug('a'.repeat(30))).toBe('a'.repeat(24));
    expect(ownFolderIn('D:/notes', 'Pipi')).toBe('D:/notes/pipi');
    expect(ownFolderIn('D:/notes/', 'Pipi')).toBe('D:/notes/pipi');
    expect(ownFolderIn(String.fromCharCode(68, 58, 92, 110), 'Pipi')).toBe(String.fromCharCode(68, 58, 92, 110, 92) + 'pipi');
  });
});
