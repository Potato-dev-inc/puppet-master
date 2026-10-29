import { describe, expect, it } from 'vitest';
import { CULPRIT, UNDO_PLAN_CONFLICT, whatBrokeItResultMock } from './what-broke-it-result-fixtures';
import { scenarios } from './scenarios';

const NAMES = [
  'what-broke-it-culprit',
  'what-broke-it-culprit-light',
  'what-broke-it-several',
  'what-broke-it-not-sure',
  'what-broke-it-undo-refused',
];

describe('what broke it result scenarios', () => {
  it('are registered under their own name, on the cockpit', () => {
    for (const name of NAMES) {
      expect(scenarios[name]?.name).toBe(name);
      expect(scenarios[name]?.route).toBe('cockpit');
    }
    expect(scenarios['what-broke-it-culprit-light']?.theme).toBe('light');
  });

  it('the refused undo scenario carries a conflicting plan', () => {
    expect(scenarios['what-broke-it-undo-refused']?.whatBrokeItResult?.undoPlan).toBe(UNDO_PLAN_CONFLICT);
  });
});

describe('whatBrokeItResultMock', () => {
  it('answers the finding and leaves other commands alone', () => {
    expect(whatBrokeItResultMock(undefined, 'wb_finding', { findingId: CULPRIT.id })).toEqual({ handled: true, value: CULPRIT });
    expect(whatBrokeItResultMock(undefined, 'list_panes', {}).handled).toBe(false);
  });

  it('a refused undo throws the message, as a rejected call does', () => {
    expect(() => whatBrokeItResultMock({ undoRefusedMessage: 'Nothing was changed.' }, 'wb_undo', {})).toThrow(
      'Nothing was changed.',
    );
  });
});
