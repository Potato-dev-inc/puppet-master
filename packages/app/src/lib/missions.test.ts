import { describe, expect, it } from 'vitest';
import { MISSION_DONE, MISSION_TAMPER, MISSION_WORKING } from '../dev/mission-fixtures';
import { PANE_CODEX } from '../dev/fixtures';
import { barText, commandText, goalNamesTests, isActive, missionOfPane, parseCommand, startBlockedReason, type SheetState } from './missions';

const ready: SheetState = {
  objective: 'The cart tests pass.',
  checks: [{ name: 'Tests', command: ['npm', 'test'], source: 'package.json', on: true }],
  agents: ['claude'],
  turns: 30,
  hours: 2,
  rollback: true,
  git: true,
  activeMissions: 0,
};

describe('the start sheet', () => {
  it('says why Start is disabled, first match wins (DESIGN-MISSIONS 6.2)', () => {
    expect(startBlockedReason(ready)).toBeNull();
    expect(startBlockedReason({ ...ready, objective: '  ' })).toBe("Say what should be true when it's done.");
    expect(startBlockedReason({ ...ready, checks: [{ ...ready.checks[0], on: false }] })).toBe(
      'Pick at least one check. A mission ends only when its checks pass.',
    );
    expect(startBlockedReason({ ...ready, turns: 0, hours: 0 })).toBe('Set a budget in turns or time.');
    expect(startBlockedReason({ ...ready, git: false })).toBe('Missions need a git project, because they work in a separate copy.');
    expect(startBlockedReason({ ...ready, activeMissions: 2 })).toBe('Two missions are already running. Stop or finish one first.');
    // An empty goal is reported before anything else.
    expect(startBlockedReason({ ...ready, objective: '', git: false, turns: 0, hours: 0 })).toBe("Say what should be true when it's done.");
  });

  it('turns "Agents may change tests" on when the goal names tests', () => {
    expect(goalNamesTests('Update the tests to the new API')).toBe(true);
    expect(goalNamesTests('fix the flaky snapshot')).toBe(true);
    expect(goalNamesTests('Make checkout faster')).toBe(false);
    expect(goalNamesTests('contest results page')).toBe(false);
  });

  it('reads a typed command into an argv and back', () => {
    expect(parseCommand('npm run lint')).toEqual(['npm', 'run', 'lint']);
    expect(parseCommand('pytest -k "cart and totals"')).toEqual(['pytest', '-k', 'cart and totals']);
    expect(commandText(['pytest', '-k', 'cart and totals'])).toBe('pytest -k "cart and totals"');
  });
});

describe('mission lists', () => {
  it('finds the mission a pane works for', () => {
    expect(missionOfPane([MISSION_WORKING], PANE_CODEX)?.id).toBe(MISSION_WORKING.id);
    expect(missionOfPane([MISSION_WORKING], 'other')).toBeNull();
  });

  it('knows which missions are still going', () => {
    expect(isActive(MISSION_WORKING)).toBe(true);
    expect(isActive(MISSION_TAMPER)).toBe(true);
    expect(isActive(MISSION_DONE)).toBe(false);
  });

  it('labels the bar with tests when they were counted, else checks', () => {
    expect(barText(MISSION_WORKING)).toBe('9 of 12 pass');
    expect(barText({ ...MISSION_WORKING, now: '1 of 2 checks', passed: 1, total: 2 })).toBe('1 of 2 checks pass');
  });
});
