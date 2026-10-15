// @vitest-environment jsdom
// The palette finds the features that hide in Settings, in the shared words.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CockpitApi } from '../../hooks/useCockpit';
import * as settings from '../../lib/settings';
import { resolveSettingsTarget } from '../../lib/settings-target';
import { Palette } from './Palette';
import { settingsShortcuts } from './palette-shortcuts';
import { onOpenStartMission } from '../../lib/missions';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  Element.prototype.scrollIntoView = vi.fn();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});

const COPY = 'C:\\work\\shop\\.puppet-master\\worktrees\\7d4cb2b3-2-209c';

function cockpit(extra: Partial<CockpitApi> = {}): CockpitApi {
  const info = { id: 'p1', agent_type: 'codex', status: 'idle', cwd: COPY, title: null };
  return {
    registry: { paneList: [{ info, status: 'idle' }], panes: new Map([['p1', { info, status: 'idle' }]]) },
    launchEntries: [],
    readyAgents: new Set(),
    names: new Map([['p1', 'Codex']]),
    selectedId: null,
    mode: 'orchestration',
    theme: 'dark',
    previews: {},
    openSettings: vi.fn(async () => {}),
    notify: vi.fn(),
    ...extra,
  } as unknown as CockpitApi;
}

const rows = () => [...host.querySelectorAll<HTMLElement>('[role="option"]')];
const titleOf = (r: Element) => r.querySelector('.ck-palette-title')?.textContent;
const rowFor = (title: string) => rows().find((r) => titleOf(r) === title);

function search(text: string) {
  const input = host.querySelector<HTMLInputElement>('input')!;
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!;
  act(() => {
    setter.call(input, text);
    input.dispatchEvent(new Event('input', { bubbles: true }));
  });
}

describe('palette commands for features that hide', () => {
  it('lists Simple mode, Live updates, Budgets, Team, Pair a phone and Save a diagnostic file', () => {
    act(() => root.render(<Palette c={cockpit()} onClose={vi.fn()} />));
    for (const title of ['Switch to Simple mode', 'Live updates…', 'Budgets…', 'Team…', 'Pair a phone…', 'Save a diagnostic file…']) {
      expect(rowFor(title), title).toBeTruthy();
    }
  });

  it.each([
    ['Pair a phone…', 'pairing'],
    ['Live updates…', 'live-updates'],
    ['Budgets…', 'budget-turns'],
    ['Team…', 'team'],
    ['Save a diagnostic file…', 'diagnostic-file'],
  ])('"%s" opens Settings on its row (%s) and closes the palette', (title, tab) => {
    const c = cockpit();
    const onClose = vi.fn();
    act(() => root.render(<Palette c={c} onClose={onClose} />));
    act(() => rowFor(title)!.click());
    expect(c.openSettings).toHaveBeenCalledWith(tab);
    expect(onClose).toHaveBeenCalled();
  });

  it.each([
    ['live-updates', 'general'],
    ['budget-turns', 'general'],
    ['team', 'general'],
    ['pairing', 'phone'],
    ['diagnostic-file', 'general'],
  ])('the row %s is a real Settings row (in %s)', (row, page) => {
    expect(resolveSettingsTarget(row)).toEqual({ page, row });
  });

  it('has one Simple mode command, next to the welcome flow\'s "Welcome tour"', () => {
    act(() => root.render(<Palette c={cockpit()} onClose={vi.fn()} onOpenWelcome={vi.fn()} />));
    expect(rows().filter((r) => /simple mode/i.test(titleOf(r) ?? ''))).toHaveLength(1);
    expect(rowFor('Welcome tour')).toBeTruthy();
  });

  it('Missions: with the Labs switch off, "Missions…" opens the switch; with it on, "Start a mission…" opens the sheet', () => {
    const c = cockpit();
    act(() => root.render(<Palette c={c} onClose={vi.fn()} />));
    act(() => rowFor('Missions…')!.click());
    expect(c.openSettings).toHaveBeenCalledWith('missions');
    expect(resolveSettingsTarget('missions')).toEqual({ page: 'general', row: 'missions' });
    const heard: (string | null)[] = [];
    const off = onOpenStartMission((p) => heard.push(p));
    const on = settingsShortcuts(c, { missionsOn: true, projectPath: 'C:\work\shop' });
    expect(on.some((e) => e.title === 'Missions…')).toBe(false);
    on.find((e) => e.title === 'Start a mission…')!.run();
    off();
    expect(heard).toEqual(['C:\work\shop']);
  });

  it('"Switch to Simple mode" turns the setting on', () => {
    const patch = vi.spyOn(settings, 'patchSettings').mockResolvedValue({} as never);
    act(() => root.render(<Palette c={cockpit()} onClose={vi.fn()} />));
    act(() => rowFor('Switch to Simple mode')!.click());
    expect(patch).toHaveBeenCalledWith({ simple_mode: true });
  });

  it('is found by the words people use, not only by the title', () => {
    act(() => root.render(<Palette c={cockpit()} onClose={vi.fn()} />));
    search('qr');
    expect(rows().map(titleOf)).toEqual(['Pair a phone…']);
    search('bug report');
    expect(rowFor('Save a diagnostic file…')).toBeTruthy();
    search('limit');
    expect(rowFor('Budgets…')).toBeTruthy();
    search('heads-up');
    expect(rowFor('Team…')).toBeTruthy();
  });

  it('says "Stop" in the shared words and names a separate copy by its project', () => {
    act(() => root.render(<Palette c={cockpit({ selectedId: 'p1' })} onClose={vi.fn()} onStopAll={vi.fn()} />));
    expect(rowFor('Stop this agent')).toBeTruthy();
    expect(host.textContent).not.toContain('Interrupt');
    const pane = rows().find((r) => r.textContent?.includes('separate copy'))!;
    expect(pane.textContent).toContain('shop · separate copy');
    expect(pane.textContent).not.toContain('7d4cb2b3');
  });

  it('calls the Project view "Project", still found by "memory"', () => {
    act(() => root.render(<Palette c={cockpit()} onClose={vi.fn()} onOpenMemory={vi.fn()} />));
    expect(rowFor('Project…')).toBeTruthy();
    search('memory');
    expect(rowFor('Project…')).toBeTruthy();
  });
});
