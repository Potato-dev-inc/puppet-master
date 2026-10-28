import { describe, expect, it } from 'vitest';
import type { PickView, Target } from '../../lib/point-api';
import { MAX_REQUEST, boxKey, groupTargets, sourceLine, targetLabel } from './point-model';

function pick(over: Partial<PickView> = {}): PickView {
  return {
    id: 'pick-1',
    page: 'localhost:5173/pricing',
    title: 'Pricing',
    label: 'button.card-cta',
    text: 'Buy',
    ...over,
  };
}

function target(over: Partial<Target> = {}): Target {
  return { paneId: 'p1', agentLabel: 'Claude Code', title: 'fix-pricing', sameFolder: false, canSend: true, ...over };
}

describe('sourceLine', () => {
  it('joins component and file with a line', () => {
    expect(sourceLine(pick({ component: 'PricingCard', source: { relPath: 'src/components/PricingCard.tsx', line: 42 } }))).toBe(
      'PricingCard · src/components/PricingCard.tsx:42',
    );
  });

  it('shows the file without a line when the line is missing or zero', () => {
    expect(sourceLine(pick({ source: { relPath: 'src/a.tsx', line: null } }))).toBe('src/a.tsx');
    expect(sourceLine(pick({ source: { relPath: 'src/a.tsx', line: 0 } }))).toBe('src/a.tsx');
  });

  it('shows only the component when there is no file', () => {
    expect(sourceLine(pick({ component: 'PricingCard', source: null }))).toBe('PricingCard');
  });

  it('shows only the file when there is no component', () => {
    expect(sourceLine(pick({ component: null, source: { relPath: 'src/a.tsx', line: 3 } }))).toBe('src/a.tsx:3');
  });

  it('returns null when the page gave neither', () => {
    expect(sourceLine(pick())).toBeNull();
    expect(sourceLine(pick({ component: '', source: null }))).toBeNull();
  });
});

describe('targetLabel', () => {
  it('shows the agent label and the pane title after a dot', () => {
    expect(targetLabel({ agentLabel: 'Claude Code', title: 'fix-pricing' })).toBe('Claude Code · fix-pricing');
  });

  it('leaves the title out when it repeats the agent label', () => {
    expect(targetLabel({ agentLabel: 'Claude Code', title: 'Claude Code' })).toBe('Claude Code');
  });

  it('treats a title that differs only by case or outer spaces as the same name', () => {
    expect(targetLabel({ agentLabel: 'Claude Code', title: 'claude code' })).toBe('Claude Code');
    expect(targetLabel({ agentLabel: ' Codex ', title: 'Codex ' })).toBe('Codex');
  });

  it('shows only the agent label when the pane has no title', () => {
    expect(targetLabel({ agentLabel: 'Codex', title: '' })).toBe('Codex');
    expect(targetLabel({ agentLabel: 'Codex', title: '   ' })).toBe('Codex');
  });
});

describe('groupTargets', () => {
  it('puts same-folder panes first and keeps the backend order inside a group', () => {
    const groups = groupTargets([
      target({ paneId: 'a', sameFolder: false }),
      target({ paneId: 'b', sameFolder: true }),
      target({ paneId: 'c', sameFolder: false }),
      target({ paneId: 'd', sameFolder: true }),
    ]);
    expect(groups.map((g) => g.label)).toEqual(['Same folder', 'Other panes']);
    expect(groups[0].items.map((t) => t.paneId)).toEqual(['b', 'd']);
    expect(groups[1].items.map((t) => t.paneId)).toEqual(['a', 'c']);
  });

  it('leaves out an empty group', () => {
    expect(groupTargets([target({ sameFolder: true })]).map((g) => g.label)).toEqual(['Same folder']);
    expect(groupTargets([target({ sameFolder: false })]).map((g) => g.label)).toEqual(['Other panes']);
  });

  it('returns no groups for no targets', () => {
    expect(groupTargets([])).toEqual([]);
  });

  it('keeps refused panes in their group so the box can grey them out', () => {
    const [group] = groupTargets([target({ paneId: 'x', sameFolder: true, canSend: false, reason: 'Shells take no messages.' })]);
    expect(group.items[0]).toMatchObject({ paneId: 'x', canSend: false, reason: 'Shells take no messages.' });
  });
});

describe('boxKey', () => {
  const key = (k: string, over: { shiftKey?: boolean; isComposing?: boolean } = {}) => ({
    key: k,
    shiftKey: false,
    isComposing: false,
    ...over,
  });

  it('Enter sends', () => {
    expect(boxKey(key('Enter'))).toBe('send');
  });

  it('Shift+Enter is a new line, not a send', () => {
    expect(boxKey(key('Enter', { shiftKey: true }))).toBeNull();
  });

  it('Escape hides the box (Esc too, for older browsers)', () => {
    expect(boxKey(key('Escape'))).toBe('hide');
    expect(boxKey(key('Esc'))).toBe('hide');
  });

  it('does nothing while an input method is composing', () => {
    expect(boxKey(key('Enter', { isComposing: true }))).toBeNull();
    expect(boxKey(key('Escape', { isComposing: true }))).toBeNull();
  });

  it('ignores every other key', () => {
    expect(boxKey(key('a'))).toBeNull();
    expect(boxKey(key('Tab'))).toBeNull();
  });
});

describe('MAX_REQUEST', () => {
  it('is 2000 characters', () => {
    expect(MAX_REQUEST).toBe(2000);
  });
});
