// The Keep going harness scenarios: they are registered, and the mocked commands answer the shapes the UI reads.
import { describe, expect, it } from 'vitest';
import { PANE_CLAUDE, PANE_CODEX } from './fixtures';
import { BRIEF, FAILED_LINEAGE, RUNNING_LINEAGE, TARGETS, keepGoingMock } from './keep-going-fixtures';
import { LIMIT_CARD, LIMIT_CARD_NO_OFFER } from './limit-fixtures';
import { scenarios } from './scenarios';

const NAMES = [
  'keep-going-card',
  'keep-going-card-light',
  'keep-going-sheet',
  'keep-going-sheet-light',
  'keep-going-banner',
  'keep-going-banner-new',
  'keep-going-banner-failed',
  'keep-going-brief-sheet',
];

describe('keep going scenarios', () => {
  it('are all registered under their own name', () => {
    for (const name of NAMES) expect(scenarios[name]?.name).toBe(name);
  });

  it('banner scenarios select the pane they show and bring their lineage', () => {
    expect(scenarios['keep-going-banner'].select).toBe(PANE_CLAUDE);
    expect(scenarios['keep-going-banner-new'].select).toBe(PANE_CODEX);
    expect(scenarios['keep-going-banner'].keepGoing?.lineage).toBe(RUNNING_LINEAGE);
    expect(scenarios['keep-going-banner-failed'].keepGoing?.lineage).toBe(FAILED_LINEAGE);
  });
});

describe('keepGoingMock', () => {
  it('leaves other commands alone', () => {
    expect(keepGoingMock(undefined, 'list_panes', {}).handled).toBe(false);
    expect(keepGoingMock(undefined, 'keep_going_nope', {}).handled).toBe(false);
  });

  it('answers the picker rows, the settings, the brief and a started hand-over', () => {
    expect(keepGoingMock(undefined, 'keep_going_targets', { paneId: 'p' }).value).toBe(TARGETS);
    expect(keepGoingMock(undefined, 'keep_going_brief', { id: 'x' }).value).toBe(BRIEF);
    expect(keepGoingMock(undefined, 'keep_going_settings_get', {}).value).toMatchObject({ ask_back: true });
    expect(keepGoingMock(undefined, 'keep_going_continue', {}).value).toMatchObject({ state: 'starting' });
  });

  it('answers the lineage by pane, and no hand-over for an unknown pane', () => {
    const s = { lineage: RUNNING_LINEAGE };
    expect(keepGoingMock(s, 'keep_going_lineage', { paneId: PANE_CODEX }).value).toEqual(RUNNING_LINEAGE[PANE_CODEX]);
    expect(keepGoingMock(s, 'keep_going_lineage', { paneId: 'other' }).value).toEqual({ from: null, to: null });
  });
});

describe('limit fixtures', () => {
  it('carry the current backend words, not the old "coming in a later update" line', () => {
    expect(LIMIT_CARD.choices[0]).toEqual({ id: 'continue:codex', label: 'Continue on Codex' });
    expect(LIMIT_CARD.context).toBe('Codex gets a short brief and works in the same folder. Claude Code in this pane is stopped first.');
    expect(JSON.stringify([LIMIT_CARD, LIMIT_CARD_NO_OFFER])).not.toContain('later update');
    expect(LIMIT_CARD_NO_OFFER.choices.map((c) => c.id)).toEqual(['remind', 'stop']);
  });
});
