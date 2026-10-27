import { describe, expect, it } from 'vitest';
import { descendants, groupHelperTree, helperChipText, helperOfLabel, liveParentId, type HelperNode } from './pane-helpers';

type P = { info: { id: string; parent_pane_id?: string | null }; status: string };
const pane = (id: string, status = 'idle', parent?: string): P => ({ info: { id, parent_pane_id: parent }, status });
const ids = (nodes: HelperNode<P>[]): string[] => nodes.map((n) => n.pane.info.id);

describe('groupHelperTree', () => {
  it('keeps panes without a parent at top level, in their status groups', () => {
    const g = groupHelperTree([pane('a', 'running'), pane('b', 'idle'), pane('c', 'waiting_input'), pane('d', 'error')]);
    expect(ids(g.working)).toEqual(['a']);
    expect(ids(g.idle)).toEqual(['b']);
    expect(ids(g.needs)).toEqual(['c', 'd']);
  });

  it('nests a helper under its parent whatever its own status', () => {
    const g = groupHelperTree([pane('p', 'running'), pane('h1', 'running', 'p'), pane('h2', 'idle', 'p')]);
    expect(ids(g.working)).toEqual(['p']);
    expect(ids(g.idle)).toEqual([]);
    expect(ids(g.working[0].helpers)).toEqual(['h1', 'h2']);
    expect(helperChipText(g.working[0])).toBe('2 helpers');
  });

  it('puts the parent first even when the helper is listed before it', () => {
    const g = groupHelperTree([pane('h', 'running', 'p'), pane('p', 'idle')]);
    expect(ids(g.idle)).toEqual(['p']);
    expect(ids(g.idle[0].helpers)).toEqual(['h']);
    expect(g.working).toEqual([]);
  });

  it('shows a helper whose parent is gone at top level', () => {
    const g = groupHelperTree([pane('h', 'running', 'closed-parent')]);
    expect(ids(g.working)).toEqual(['h']);
    expect(g.working[0].helpers).toEqual([]);
  });

  it('lists a helper that needs you in Needs you as well, away from its parent', () => {
    const parent = pane('p', 'running');
    const g = groupHelperTree([parent, pane('h', 'waiting_input', 'p')]);
    expect(ids(g.working[0].helpers)).toEqual(['h']);
    expect(ids(g.needs)).toEqual(['h']);
    expect(g.needs[0].awayFrom).toBe(parent);
    expect(g.needs[0].helpers).toEqual([]);
  });

  it('does not list a helper twice when its parent already sits in Needs you', () => {
    const g = groupHelperTree([pane('p', 'waiting_input'), pane('h', 'error', 'p')]);
    expect(ids(g.needs)).toEqual(['p']);
    expect(ids(g.needs[0].helpers)).toEqual(['h']);
  });

  it('counts an open question as needing you for helpers too', () => {
    const g = groupHelperTree([pane('p', 'idle'), pane('h', 'running', 'p')], new Set(['h']));
    expect(ids(g.needs)).toEqual(['h']);
    expect(ids(g.idle)).toEqual(['p']);
  });

  it('nests helpers of helpers and counts them all in the chip', () => {
    const g = groupHelperTree([pane('p', 'running'), pane('h', 'running', 'p'), pane('hh', 'idle', 'h')]);
    expect(ids(g.working)).toEqual(['p']);
    expect(ids(g.working[0].helpers[0].helpers)).toEqual(['hh']);
    expect(descendants(g.working[0]).map((n) => n.pane.info.id)).toEqual(['h', 'hh']);
    expect(helperChipText(g.working[0])).toBe('2 helpers');
    expect(helperChipText(g.working[0].helpers[0])).toBe('1 helper');
  });

  it('promotes a deep helper that needs you without its own children', () => {
    const g = groupHelperTree([pane('p', 'running'), pane('h', 'running', 'p'), pane('hh', 'waiting_input', 'h')]);
    expect(ids(g.needs)).toEqual(['hh']);
    expect(g.needs[0].awayFrom?.info.id).toBe('h');
  });

  it('survives a malformed loop and a pane naming itself', () => {
    const g = groupHelperTree([pane('a', 'idle', 'b'), pane('b', 'idle', 'a'), pane('c', 'idle', 'c')]);
    expect(ids(g.idle).sort()).toEqual(['a', 'b', 'c']);
  });
});

describe('helperOfLabel', () => {
  const list = [pane('p'), pane('h', 'idle', 'p'), pane('lost', 'idle', 'gone'), pane('solo')];
  const title = (p: { info: { id: string } }) => `Title ${p.info.id}`;
  it('names the live parent', () => {
    expect(helperOfLabel(list, 'h', title)).toEqual({ parentId: 'p', text: 'Helper of Title p' });
  });
  it('is null without a live parent', () => {
    expect(helperOfLabel(list, 'lost', title)).toBeNull();
    expect(helperOfLabel(list, 'solo', title)).toBeNull();
    expect(helperOfLabel(list, 'nope', title)).toBeNull();
    expect(liveParentId(list, 'h')).toBe('p');
  });
});
