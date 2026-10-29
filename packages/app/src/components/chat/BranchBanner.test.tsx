// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from 'vitest';
import { branchBrief, branchLineage, onBranchChanged, type BranchLineage, type BranchView } from '../../lib/branch-from';
import { BranchBanner, branchSuffix } from './BranchBanner';

vi.mock('../../lib/branch-from', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../lib/branch-from')>();
  return { ...actual, branchLineage: vi.fn(), branchBrief: vi.fn(), onBranchChanged: vi.fn() };
});

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// Fake labels only. The banner shows whatever label the record carries.
const view = (patch: Partial<BranchView> = {}): BranchView => ({
  id: 'bf-0123456789ab',
  fromLabel: 'Fake A',
  toLabel: 'Fake B · Work',
  fromPaneId: 'pane-a',
  toPaneId: 'pane-b',
  atMs: 1_000,
  pointLabel: 'after turn 5 · 14:02',
  state: 'running',
  stateText: 'Running in a separate copy',
  turns: 5,
  briefBytes: 321,
  branchName: 'pm/bf-0123456789ab',
  closed: null,
  failedFiles: 0,
  ...patch,
});

const lineageMock = branchLineage as Mock;
const briefMock = branchBrief as Mock;
const listenMock = onBranchChanged as Mock;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  lineageMock.mockReset();
  briefMock.mockReset();
  listenMock.mockReset();
  listenMock.mockResolvedValue(() => {});
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});

async function mount(paneId: string, lineage: BranchLineage | Error, onOpenPane?: (paneId: string) => void) {
  if (lineage instanceof Error) lineageMock.mockRejectedValue(lineage);
  else lineageMock.mockResolvedValue(lineage);
  await act(async () => {
    root.render(<BranchBanner paneId={paneId} onOpenPane={onOpenPane} />);
  });
}
const button = (text: string) => Array.from(host.querySelectorAll('button')).find((b) => b.textContent?.trim() === text);
const rowOf = (kind: string) => host.querySelector<HTMLElement>(`[data-branch-banner="${kind}"]`);

describe('BranchBanner: new pane (branched from)', () => {
  it('shows the source and save point, Show brief and Open original; Open original opens the source pane', async () => {
    const onOpenPane = vi.fn();
    await mount('pane-b', { from: view(), children: [] }, onOpenPane);
    expect(rowOf('new')!.textContent).toBe('Branched from Fake A · after turn 5 · 14:02·Show briefOpen original');
    await act(async () => button('Open original')!.click());
    expect(onOpenPane).toHaveBeenCalledWith('pane-a');
  });

  it('Show brief loads the stored brief through branchBrief and shows it in the sheet', async () => {
    briefMock.mockResolvedValue('Branch: try the same fix with a different helper.');
    await mount('pane-b', { from: view(), children: [] });
    await act(async () => button('Show brief')!.click());
    expect(briefMock).toHaveBeenCalledWith('bf-0123456789ab');
    expect(host.querySelector('[role="dialog"]')!.textContent).toContain('The brief Fake B · Work got');
    expect(host.querySelector('pre')!.textContent).toBe('Branch: try the same fix with a different helper.');
  });

  it('shows a plain error without the code when the brief will not load', async () => {
    briefMock.mockRejectedValue(new Error('BRIEF_MISSING: no brief is stored for this branch'));
    await mount('pane-b', { from: view(), children: [] });
    await act(async () => button('Show brief')!.click());
    expect(host.querySelector('[role="alert"]')!.textContent).toBe('No brief is stored for this branch');
    expect(host.textContent).not.toContain('BRIEF_MISSING');
  });

  it('hides Open original when no pane-focus callback is given', async () => {
    await mount('pane-b', { from: view(), children: [] });
    expect(rowOf('new')).toBeTruthy();
    expect(button('Open original')).toBeUndefined();
  });
});

describe('BranchBanner: source pane (branches)', () => {
  it('lists both branches, and each name opens its own pane', async () => {
    const onOpenPane = vi.fn();
    const children = [
      view({ id: 'bf-aaaaaaaaaaaa', toLabel: 'Fake B · Work', toPaneId: 'pane-b' }),
      view({ id: 'bf-bbbbbbbbbbbb', toLabel: 'Fake C', toPaneId: 'pane-c' }),
    ];
    await mount('pane-a', { from: null, children }, onOpenPane);
    expect(rowOf('source')!.textContent).toBe('Branches:Fake B · Work·Fake C');
    await act(async () => button('Fake C')!.click());
    expect(onOpenPane).toHaveBeenLastCalledWith('pane-c');
    await act(async () => button('Fake B · Work')!.click());
    expect(onOpenPane).toHaveBeenLastCalledWith('pane-b');
  });

  it('marks a branch that could not start, shows it without a button, and names the failure', async () => {
    const failed = view({ id: 'bf-cccccccccccc', toLabel: 'Fake D', toPaneId: '', state: 'failed', stateText: "Fake D couldn't start. The copy is kept." });
    await mount('pane-a', { from: null, children: [failed] }, vi.fn());
    expect(rowOf('source')!.textContent).toBe("Branches:Fake D (couldn't start)");
    expect(button("Fake D (couldn't start)")).toBeUndefined();
    expect(rowOf('failed')!.textContent).toBe("Branch couldn't start: Fake D couldn't start. The copy is kept.");
  });

  it('says whether a branch was kept or thrown away', async () => {
    await mount('pane-a', {
      from: null,
      children: [
        view({ id: 'bf-dddddddddddd', toLabel: 'Fake E', closed: 'kept' }),
        view({ id: 'bf-eeeeeeeeeeee', toLabel: 'Fake F', closed: 'thrown_away' }),
      ],
    });
    expect(rowOf('source')!.textContent).toBe('Branches:Fake E (kept)·Fake F (thrown away)');
  });

  it('shows the making line while a branch copy is being made', async () => {
    await mount('pane-a', {
      from: null,
      children: [view({ id: 'bf-ffffffffffff', toLabel: 'Fake G', toPaneId: '', state: 'preparing', stateText: '' })],
    });
    expect(rowOf('making')!.textContent).toBe('Making a copy from the save point…');
    expect(rowOf('source')!.textContent).toBe('Branches:Fake G');
  });

  it('branchSuffix is empty for an open branch', () => {
    expect(branchSuffix(view())).toBe('');
  });
});

describe('BranchBanner: partial copy', () => {
  it('says how many files could not be written, in the plural', async () => {
    await mount('pane-b', { from: view({ failedFiles: 3 }), children: [] });
    expect(rowOf('partial')!.textContent).toBe("3 files couldn't be written into the copy");
  });

  it('uses the singular for one file', async () => {
    await mount('pane-b', { from: view({ failedFiles: 1 }), children: [] });
    expect(rowOf('partial')!.textContent).toBe("1 file couldn't be written into the copy");
  });
});

describe('BranchBanner: nothing, changes and errors', () => {
  it('renders nothing for a pane with no branches', async () => {
    await mount('pane-a', { from: null, children: [] });
    expect(host.innerHTML).toBe('');
  });

  it('renders nothing when the lineage cannot be read', async () => {
    await mount('pane-a', new Error('desktop only'));
    expect(host.innerHTML).toBe('');
  });

  it('reloads when branch://changed names this pane, and ignores other panes', async () => {
    let fire: (e: { paneIds: string[] }) => void = () => {};
    listenMock.mockImplementation(async (cb: (e: { paneIds: string[] }) => void) => {
      fire = cb;
      return () => {};
    });
    await mount('pane-a', { from: null, children: [view({ toLabel: 'Fake C' })] });
    expect(lineageMock).toHaveBeenCalledTimes(1);
    await act(async () => fire({ paneIds: ['some-other-pane'] }));
    expect(lineageMock).toHaveBeenCalledTimes(1);
    lineageMock.mockResolvedValue({ from: null, children: [view({ toLabel: 'Fake H' })] });
    await act(async () => fire({ paneIds: ['pane-x', 'pane-a'] }));
    expect(lineageMock).toHaveBeenCalledTimes(2);
    expect(rowOf('source')!.textContent).toBe('Branches:Fake H');
  });

  it('still shows the rows when the event channel is not there', async () => {
    listenMock.mockRejectedValue(new Error('no events'));
    await mount('pane-b', { from: view(), children: [] });
    expect(rowOf('new')).toBeTruthy();
  });

  it('stops listening when it unmounts', async () => {
    const stop = vi.fn();
    listenMock.mockResolvedValue(stop);
    await mount('pane-b', { from: view(), children: [] });
    act(() => root.unmount());
    root = createRoot(host);
    await act(async () => {});
    expect(stop).toHaveBeenCalledTimes(1);
  });
});
