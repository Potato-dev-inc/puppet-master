import { describe, expect, it } from 'vitest';
import { companionIdOf, companionTab, isCompanionPane, isCompanionTab } from './mobile-agent-tabs';

describe('companion tab ids', () => {
  it('a companion tab is companion:<id>, and only those are companion tabs', () => {
    expect(companionTab('mochi')).toBe('companion:mochi');
    expect(isCompanionTab('companion:mochi')).toBe(true);
    expect(companionIdOf('companion:mochi')).toBe('mochi');
    expect(isCompanionTab('all')).toBe(false);
    expect(isCompanionTab('companion-mochi')).toBe(false);
    expect(companionIdOf('pane-1')).toBeNull();
  });

  it('a companion pane is companion-<id> on the PC, and an agent pane is not', () => {
    expect(isCompanionPane('companion-mochi')).toBe(true);
    expect(isCompanionPane('a1b2c3')).toBe(false);
    expect(isCompanionPane('all')).toBe(false);
  });
});
