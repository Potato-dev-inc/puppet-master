// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AgentStoreApi, StoreCard, StoreList, StoreState } from '../../lib/agent-store';

const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invoke(...args) }));

import { AgentStore } from './AgentStore';
import { AgentStoreCard, stateGlyph } from './AgentStoreCard';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  invoke.mockReset();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

function card(over: Partial<StoreCard> & { id: string; label: string }): StoreCard {
  return {
    icon: 'AB',
    tagline: 'A short line about the agent.',
    tier: 'starter',
    builtin: false,
    categories: ['general'],
    status: 'active',
    replaced_by: '',
    vendor: {
      name: 'Acme Labs',
      homepage: 'https://acme.example',
      license: 'MIT',
      license_kind: 'open_source',
      license_url: null,
      terms_url: null,
    },
    sign_in: { kind: 'none', summary: 'No sign-in needed.' },
    checked: { date: '2026-10-28', version: '1.0.0', how: 'vendor_docs', windows_tested: false },
    state: 'available',
    state_reason: 'Not installed yet.',
    installed_version: null,
    modes: ['Pane'],
    can_install: true,
    ambiguous_name: false,
    last_check: null,
    ...over,
  };
}

/** A fake api: `list` is the only call the grid makes. The rest reject if called. */
function fakeApi(list: () => Promise<StoreList>): AgentStoreApi {
  const unused = () => Promise.reject(new Error('not used by the grid'));
  return {
    list,
    detail: unused,
    add: unused,
    remove: unused,
    recordInstall: unused,
    uninstallPlan: unused,
    uninstallRun: unused,
    recordCheck: unused,
  } as AgentStoreApi;
}

const listOf = (entries: StoreCard[]) => () => Promise.resolve({ catalog_version: 'test1234', entries });

const SAMPLE: StoreCard[] = [
  card({ id: 'alpha', label: 'Alpha Code', state: 'available', tier: 'verified', tagline: 'Writes code in a pane.' }),
  card({
    id: 'beta',
    label: 'Beta Chat',
    state: 'ready',
    tier: 'verified',
    tagline: 'Chat with a key.',
    sign_in: { kind: 'api_key', summary: 'Paste your key in the agent.' },
    vendor: { name: 'Beta Co', homepage: 'https://beta.example', license: 'Paid', license_kind: 'proprietary', license_url: null, terms_url: null },
  }),
  card({
    id: 'gamma',
    label: 'Gamma Local',
    state: 'found_on_pc',
    tier: 'starter',
    tagline: 'Runs models on this PC.',
    vendor: { name: 'Gamma Labs', homepage: 'https://gamma.example', license: 'Apache-2.0', license_kind: 'source_available', license_url: null, terms_url: null },
  }),
];

const flush = () =>
  act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });

async function mount(api: AgentStoreApi, onOpenAgent = vi.fn(), onBack = vi.fn()) {
  await act(async () => {
    root.render(<AgentStore api={api} onOpenAgent={onOpenAgent} onBack={onBack} />);
  });
  await flush();
  return { onOpenAgent, onBack };
}

const titles = () => Array.from(host.querySelectorAll('.ck-store-card .ck-row-title')).map((el) => el.textContent);
const cardOf = (label: string) =>
  Array.from(host.querySelectorAll('.ck-store-card')).find((el) => el.querySelector('.ck-row-title')?.textContent === label) as
    | HTMLElement
    | undefined;
const chip = (label: string) =>
  Array.from(host.querySelectorAll<HTMLButtonElement>('.ck-store-chips button')).find((b) => b.textContent === label) as HTMLButtonElement;
const search = () => host.querySelector('input.ck-store-search') as HTMLInputElement;

async function type(value: string) {
  await act(async () => {
    const setValue = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!;
    setValue.call(search(), value);
    search().dispatchEvent(new Event('input', { bubbles: true }));
  });
}

async function press(el: HTMLElement) {
  await act(async () => {
    el.click();
  });
}

describe('AgentStore grid', () => {
  it('renders one card per agent from the api, ready ones first', async () => {
    await mount(fakeApi(listOf(SAMPLE)));
    expect(host.querySelector('h1')?.textContent).toBe('Agent Store');
    expect(titles()).toEqual(['Beta Chat', 'Gamma Local', 'Alpha Code']);
    expect(host.querySelector('.ck-store-grid')).not.toBeNull();
  });

  it('search matches the name, tagline and vendor, ignoring case', async () => {
    await mount(fakeApi(listOf(SAMPLE)));
    await type('PANE');
    expect(titles()).toEqual(['Alpha Code']);
    await type('gamma labs');
    expect(titles()).toEqual(['Gamma Local']);
    await type('');
    expect(titles()).toHaveLength(3);
  });

  it('shows the empty message when nothing matches the search', async () => {
    await mount(fakeApi(listOf(SAMPLE)));
    await type('zzz-no-such-agent');
    expect(titles()).toEqual([]);
    expect(host.querySelector('.ck-store-empty')?.textContent).toBe('No agents match. Try another word, or press All.');
  });

  it('filter chips show only matching cards, and All brings them back', async () => {
    await mount(fakeApi(listOf(SAMPLE)));

    await press(chip('Ready'));
    expect(titles()).toEqual(['Beta Chat']);
    expect(chip('Ready').getAttribute('aria-pressed')).toBe('true');
    expect(chip('All').getAttribute('aria-pressed')).toBe('false');

    await press(chip('Open source'));
    expect(titles()).toEqual(['Alpha Code']);

    await press(chip('Needs a key'));
    expect(titles()).toEqual(['Beta Chat']);

    await press(chip('Tested'));
    expect(titles()).toEqual(['Beta Chat', 'Alpha Code']);

    await press(chip('All'));
    expect(titles()).toHaveLength(3);
  });

  it('gives each state its one primary button, and calls onOpenAgent with the id', async () => {
    const states: [StoreState, boolean, string | null][] = [
      ['available', true, 'Install'],
      ['available', false, 'How to install'],
      ['added_no_program', true, 'Install'],
      ['found_on_pc', true, 'Add'],
      ['ready', true, 'Open'],
      ['needs_review', true, 'Review'],
      ['own_version', true, null],
      ['unsupported', true, null],
    ];
    const entries = states.map(([state, canInstall], i) =>
      card({ id: `s${i}`, label: `State ${i}`, state, can_install: canInstall }),
    );
    const { onOpenAgent } = await mount(fakeApi(listOf(entries)));

    states.forEach(([, , label], i) => {
      const cardEl = cardOf(`State ${i}`)!;
      const button = cardEl.querySelector('button.ck-btn') as HTMLButtonElement | null;
      if (label === null) {
        expect(button, `state ${i}`).toBeNull();
      } else {
        expect(button?.textContent, `state ${i}`).toBe(label);
      }
    });

    const install = cardOf('State 0')!.querySelector('button') as HTMLButtonElement;
    await press(install);
    expect(onOpenAgent).toHaveBeenCalledWith('s0');
  });

  it('shows the tier and license chips in plain words', async () => {
    await mount(fakeApi(listOf(SAMPLE)));
    const beta = cardOf('Beta Chat')!;
    expect(beta.querySelector('.ck-store-tag')?.textContent).toBe('Tested');
    expect(Array.from(beta.querySelectorAll('.ck-store-tag')).map((t) => t.textContent)).toEqual(['Tested', 'Paid terms']);

    const gamma = cardOf('Gamma Local')!;
    expect(Array.from(gamma.querySelectorAll('.ck-store-tag')).map((t) => t.textContent)).toEqual([
      'Basic support',
      'Source available',
    ]);
    expect(cardOf('Alpha Code')!.querySelector('.ck-store-tag')?.textContent).toBe('Tested');
  });

  it('maps states to the DotSpinner glyph: check, plus, cross, idle', () => {
    expect(stateGlyph('ready')).toBe('exited');
    expect(stateGlyph('needs_review')).toBe('waiting_input');
    expect(stateGlyph('added_no_program')).toBe('error');
    expect(stateGlyph('available')).toBe('idle');
    expect(stateGlyph('unsupported')).toBe('idle');
  });

  it('shows the state reason under the card', async () => {
    await mount(fakeApi(listOf([card({ id: 'x', label: 'Xeno', state_reason: 'Found at a path on this PC.' })])));
    expect(cardOf('Xeno')!.querySelector('.ck-store-reason')?.textContent).toBe('Found at a path on this PC.');
  });

  it('shows the error banner when list rejects, and Try again loads the list', async () => {
    const list = vi
      .fn()
      .mockRejectedValueOnce(new Error('AGENT_STORE_BROKEN: catalog is missing'))
      .mockImplementationOnce(listOf(SAMPLE));
    await mount(fakeApi(list));

    const banner = host.querySelector('[role="alert"]') as HTMLElement;
    expect(banner.textContent).toContain('Could not load the Agent Store: catalog is missing');
    expect(titles()).toEqual([]);

    await press(banner.querySelector('button') as HTMLButtonElement);
    await flush();
    expect(list).toHaveBeenCalledTimes(2);
    expect(host.querySelector('[role="alert"]')).toBeNull();
    expect(titles()).toHaveLength(3);
  });

  it('shows the plain empty line when the Store has no agents', async () => {
    await mount(fakeApi(listOf([])));
    expect(host.querySelector('.ck-store-empty')?.textContent).toBe('The Store has no agents yet.');
  });

  it('shows catalog text as plain text: no HTML, no links', async () => {
    const hostile = card({
      id: 'evil',
      label: 'Evil <b>Name</b>',
      tagline: '<img src=x onerror="alert(1)"> see https://evil.example now',
    });
    await mount(fakeApi(listOf([hostile])));
    const cardEl = cardOf('Evil <b>Name</b>')!;
    expect(cardEl.querySelector('img, a, b')).toBeNull();
    expect(cardEl.querySelector('.ck-store-tagline')?.textContent).toBe('<img src=x onerror="alert(1)"> see https://evil.example now');
    expect(host.querySelector('a')).toBeNull();
  });

  it('Back calls onBack', async () => {
    const { onBack } = await mount(fakeApi(listOf(SAMPLE)));
    await press(host.querySelector('.ck-store-head button') as HTMLButtonElement);
    expect(onBack).toHaveBeenCalledTimes(1);
  });

  it('without an api prop it reads the list through the Tauri command', async () => {
    invoke.mockResolvedValueOnce({ catalog_version: 'abc', entries: [] });
    await act(async () => {
      root.render(<AgentStore onOpenAgent={vi.fn()} onBack={vi.fn()} />);
    });
    await flush();
    expect(invoke).toHaveBeenCalledWith('agent_store_list');
    expect(host.querySelector('.ck-store-empty')?.textContent).toBe('The Store has no agents yet.');
  });
});

describe('AgentStoreCard', () => {
  it('uses the card id for its button and labels the glyph for screen readers', async () => {
    await act(async () => {
      root.render(<AgentStoreCard card={card({ id: 'ok', label: 'Ok Agent', state: 'ready' })} onOpen={vi.fn()} />);
    });
    const glyph = host.querySelector('.ck-store-glyph [role="img"]');
    expect(glyph?.getAttribute('aria-label')).toBe('Ready');
    expect(host.querySelector('button')?.getAttribute('aria-label')).toBe('Open Ok Agent');
  });
});
