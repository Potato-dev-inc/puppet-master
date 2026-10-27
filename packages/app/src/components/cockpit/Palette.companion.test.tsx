// @vitest-environment jsdom
// The palette's companion entries: "New companion", and "Open {name}" per companion; a companion's pane is not a
// second "Panes" row.
import { act, type ComponentProps } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CockpitApi } from '../../hooks/useCockpit';
import type { CompanionView } from '../../lib/companion';
import { Palette } from './Palette';

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

function cockpit(): CockpitApi {
  const panes = [
    { info: { id: 'p1', agent_type: 'codex', status: 'idle', cwd: 'C:\\work\\shop', title: null }, status: 'idle' },
    { info: { id: 'companion-mochi', agent_type: 'codex', status: 'idle', cwd: 'C:\\work\\home', title: null }, status: 'idle' },
  ];
  return {
    registry: { paneList: panes, panes: new Map(panes.map((p) => [p.info.id, p])) },
    launchEntries: [],
    readyAgents: new Set(),
    names: new Map([
      ['p1', 'Codex'],
      ['companion-mochi', 'Mochi'],
    ]),
    selectedId: null,
    mode: 'orchestration',
    theme: 'dark',
    previews: {},
    openSettings: vi.fn(async () => {}),
    notify: vi.fn(),
  } as unknown as CockpitApi;
}

const companion: CompanionView = {
  id: 'mochi',
  name: 'Mochi',
  avatar: 'mochi',
  persona: 'calm',
  agentType: 'codex',
  agentLabel: 'Codex',
  folderName: 'home',
  state: 'idle',
  stateLine: 'Here. Ask me anything.',
  paneId: 'companion-mochi',
  needs: null,
  pendingMemory: 0,
  nextCheckin: null,
  restarts: 0,
  autoStart: true,
};

const rows = () => [...host.querySelectorAll<HTMLElement>('[role="option"]')];
const rowFor = (title: string) => rows().find((r) => r.querySelector('.ck-palette-title')?.textContent === title);

async function open(props: Partial<ComponentProps<typeof Palette>> = {}) {
  await act(async () => root.render(<Palette c={cockpit()} onClose={() => {}} {...props} />));
}

describe('palette: companions', () => {
  it('offers "New companion" and starts the setup sheet', async () => {
    const onNewCompanion = vi.fn();
    await open({ companions: [], onNewCompanion });
    const row = rowFor('New companion');
    expect(row).toBeTruthy();
    act(() => row!.click());
    expect(onNewCompanion).toHaveBeenCalledTimes(1);
  });

  it('offers "Open {name}" for each companion, and opens it', async () => {
    const onOpenCompanion = vi.fn();
    await open({ companions: [companion], onOpenCompanion });
    const row = rowFor('Open Mochi');
    expect(row).toBeTruthy();
    act(() => row!.click());
    expect(onOpenCompanion).toHaveBeenCalledWith('mochi');
  });

  it("lists the companion's pane once, as Open {name}, not as a second pane", async () => {
    await open({ companions: [companion], onOpenCompanion: vi.fn() });
    const titles = rows().map((r) => r.querySelector('.ck-palette-title')?.textContent);
    expect(titles.filter((t) => t === 'Mochi')).toHaveLength(0);
    expect(titles.filter((t) => t === 'Open Mochi')).toHaveLength(1);
  });

  it('with no companion backend, shows neither entry', async () => {
    await open({ companions: null });
    expect(rowFor('Open Mochi')).toBeUndefined();
  });
});
