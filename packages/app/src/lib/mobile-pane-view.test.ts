// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from 'vitest';
import { fallbackListing } from './agentRegistry';
import { chatAllowedFor, keepViewsOf, LS_PANE_VIEWS, readPaneViews, writePaneViews } from './mobile-pane-view';

beforeEach(() => localStorage.clear());

describe('the phone pane view choice', () => {
  it('starts empty: every pane shows its terminal', () => {
    expect(readPaneViews()).toEqual({});
  });

  it('remembers only the panes that show their chat', () => {
    writePaneViews({ a: 'chat', b: 'terminal', c: 'chat' });
    expect(JSON.parse(localStorage.getItem(LS_PANE_VIEWS)!)).toEqual({ a: 'chat', c: 'chat' });
    expect(readPaneViews()).toEqual({ a: 'chat', c: 'chat' });
  });

  it('ignores a damaged value', () => {
    localStorage.setItem(LS_PANE_VIEWS, 'not json');
    expect(readPaneViews()).toEqual({});
    localStorage.setItem(LS_PANE_VIEWS, JSON.stringify(['chat']));
    expect(readPaneViews()).toEqual({});
    localStorage.setItem(LS_PANE_VIEWS, JSON.stringify({ a: 'chat', b: 'weird', c: 3 }));
    expect(readPaneViews()).toEqual({ a: 'chat' });
  });

  it('forgets the choice of a pane that closed', () => {
    expect(keepViewsOf({ a: 'chat', b: 'chat' }, ['b', 'c'])).toEqual({ b: 'chat' });
    expect(keepViewsOf({ a: 'chat' }, ['a'])).toEqual({ a: 'chat' });
  });

  it('gives a plain shell no chat and every agent one', () => {
    const listing = fallbackListing();
    const shell = listing.agents.find((agent) => agent.kind === 'shell');
    expect(shell).toBeDefined();
    expect(chatAllowedFor(listing, shell!.id)).toBe(false);
    const agent = listing.agents.find((a) => a.kind !== 'shell')!;
    expect(chatAllowedFor(listing, agent.id)).toBe(true);
    // An agent the list does not know yet still gets the view (the PC says if it cannot).
    expect(chatAllowedFor(listing, 'not-listed')).toBe(true);
  });
});
