// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PaneInfo } from '@puppet-master/shared';
import type { CockpitApi } from '../../hooks/useCockpit';
import { Palette } from './Palette';
import { RenameTitle, type RenameTitleProps } from './RenameTitle';
import { Sidebar } from './Sidebar';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// jsdom has neither; the sidebar measures itself and the palette scrolls its active row into view.
(globalThis as { ResizeObserver?: unknown }).ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};
Element.prototype.scrollIntoView ??= () => {};

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const PROJECT_CWD = 'C:\\Users\\me\\puppet-master';

function info(id: string, title?: string | null): PaneInfo {
  return {
    id,
    agent_type: 'codex',
    pid: 1,
    status: 'idle',
    created_at: 1,
    last_output_at: null,
    cwd: PROJECT_CWD,
    cols: 80,
    rows: 24,
    title,
  } as PaneInfo;
}

/** A minimal CockpitApi: only what Sidebar and Palette read. */
function fakeApi(panes: { info: PaneInfo; status: string }[], names: Record<string, string>, selectedId: string | null = null) {
  const panesMap = new Map(panes.map((p) => [p.info.id, p]));
  return {
    registry: { paneList: panes, panes: panesMap, initialReady: true },
    names: new Map(Object.entries(names)),
    launchEntries: [],
    readyAgents: new Set<string>(),
    mode: 'orchestration',
    theme: 'dark',
    selectedId,
    select: vi.fn(),
    prompts: {},
    statusSince: new Map(panes.map((p) => [p.info.id, 0])),
    now: 60_000,
    spawn: null,
    dismissSpawn: vi.fn(),
    toasts: [],
    notify: vi.fn(),
    setMode: vi.fn(async () => {}),
    toggleTheme: vi.fn(async () => {}),
    newAgent: vi.fn(async () => {}),
    answer: vi.fn(async () => {}),
    interrupt: vi.fn(async () => {}),
    closePane: vi.fn(async () => {}),
    rename: vi.fn(async () => {}),
    openSettings: vi.fn(async () => {}),
    bridgeLine: 'Bridge test',
  } as unknown as CockpitApi;
}

/** Set a React-controlled input's value the way a user typing would, then fire the input event. */
function typeInto(input: HTMLInputElement, value: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set;
  act(() => {
    setter?.call(input, value);
    input.dispatchEvent(new Event('input', { bubbles: true }));
  });
}

function pressKey(el: HTMLElement, key: string) {
  act(() => {
    el.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }));
  });
}

describe('Sidebar pane titles', () => {
  it('shows a titled pane by its title, with the agent and project folder beneath', () => {
    const api = fakeApi([{ info: info('p1', 'auth refactor'), status: 'idle' }], { p1: 'Codex' });
    act(() => {
      root.render(<Sidebar c={api} onOpenPalette={() => {}} />);
    });
    const row = host.querySelector('.ck-row:not(.ck-row--flat)') as HTMLElement;
    expect(row.querySelector('.ck-row-title')?.textContent).toBe('auth refactor');
    const sub = row.querySelector('.ck-row-sub')?.textContent ?? '';
    expect(sub).toContain('Codex');
    expect(sub).toContain('puppet-master');
  });

  it('shows an untitled pane by its agent label, with only the project folder beneath', () => {
    const api = fakeApi([{ info: info('p1', null), status: 'idle' }], { p1: 'Codex' });
    act(() => {
      root.render(<Sidebar c={api} onOpenPalette={() => {}} />);
    });
    const row = host.querySelector('.ck-row:not(.ck-row--flat)') as HTMLElement;
    expect(row.querySelector('.ck-row-title')?.textContent).toBe('Codex');
    expect(row.querySelector('.ck-row-sub')?.textContent).toBe('puppet-master');
  });
});

describe('RenameTitle', () => {
  const baseProps = (over: Partial<RenameTitleProps> = {}): RenameTitleProps => ({
    text: 'auth refactor',
    current: 'auth refactor',
    editing: false,
    onEditingChange: vi.fn(),
    onSave: vi.fn(),
    ...over,
  });

  function render(props: RenameTitleProps) {
    act(() => {
      root.render(<RenameTitle {...props} />);
    });
  }

  it('renders the text as an h1 when not editing', () => {
    render(baseProps());
    const h1 = host.querySelector('h1');
    expect(h1?.textContent).toBe('auth refactor');
    expect(host.querySelector('input')).toBeNull();
  });

  it('starts editing on double click', () => {
    const props = baseProps();
    render(props);
    act(() => {
      host.querySelector('h1')?.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    });
    expect(props.onEditingChange).toHaveBeenCalledWith(true);
    expect(props.onSave).not.toHaveBeenCalled();
  });

  it('shows an input labelled "Pane name" holding the current title while editing', () => {
    render(baseProps({ editing: true }));
    const input = host.querySelector('input') as HTMLInputElement;
    expect(input).not.toBeNull();
    expect(input.getAttribute('aria-label')).toBe('Pane name');
    expect(input.value).toBe('auth refactor');
  });

  it('saves the cleaned text on Enter and leaves editing', () => {
    const props = baseProps({ editing: true });
    render(props);
    const input = host.querySelector('input') as HTMLInputElement;
    typeInto(input, '  new   name  ');
    pressKey(input, 'Enter');
    expect(props.onSave).toHaveBeenCalledTimes(1);
    expect(props.onSave).toHaveBeenCalledWith('new name');
    expect(props.onEditingChange).toHaveBeenCalledWith(false);
  });

  it('cancels on Escape without saving', () => {
    const props = baseProps({ editing: true });
    render(props);
    const input = host.querySelector('input') as HTMLInputElement;
    typeInto(input, 'something else');
    pressKey(input, 'Escape');
    expect(props.onEditingChange).toHaveBeenCalledWith(false);
    expect(props.onSave).not.toHaveBeenCalled();
  });

  it('does not save on Enter when the text is unchanged', () => {
    const props = baseProps({ editing: true });
    render(props);
    const input = host.querySelector('input') as HTMLInputElement;
    pressKey(input, 'Enter');
    expect(props.onSave).not.toHaveBeenCalled();
    expect(props.onEditingChange).toHaveBeenCalledWith(false);
  });

  it('clears the name when the box is emptied and Enter is pressed', () => {
    const props = baseProps({ editing: true });
    render(props);
    const input = host.querySelector('input') as HTMLInputElement;
    typeInto(input, '');
    pressKey(input, 'Enter');
    expect(props.onSave).toHaveBeenCalledWith('');
  });
});

describe('Palette pane titles', () => {
  it('shows a titled pane by its title with the agent name in the description', () => {
    const api = fakeApi([{ info: info('p1', 'auth refactor'), status: 'idle' }], { p1: 'Codex' });
    act(() => {
      root.render(<Palette c={api} onClose={() => {}} />);
    });
    const row = host.querySelector('.ck-palette-row') as HTMLElement;
    expect(row.querySelector('.ck-palette-title')?.textContent).toBe('auth refactor');
    expect(row.querySelector('.ck-palette-desc')?.textContent).toContain('Codex');
  });
});
