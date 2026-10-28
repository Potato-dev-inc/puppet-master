// @vitest-environment jsdom
// The Agent Store detail sheet: catalog text is plain, links show their full URL and open through the app's
// helper, every install, add, remove and uninstall button says what it does, and nothing runs before its confirm.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import type { AgentInstallApi, InstallPlan, InstallStatus, InstallStep } from '../../lib/agent-install';
import type { AgentStoreApi, StoreDetail, StoreState } from '../../lib/agent-store';
import { AgentDetail } from './AgentDetail';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  vi.mocked(invoke).mockReset();
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const flush = () => act(async () => {});
const text = () => host.textContent ?? '';
const button = (name: string) =>
  [...host.querySelectorAll('button')].find((b) => b.textContent?.trim() === name) as HTMLButtonElement | undefined;
const click = async (el: HTMLElement | undefined) => {
  if (!el) throw new Error('button not found');
  await act(async () => {
    el.click();
  });
};

const step = (over: Partial<InstallStep> = {}): InstallStep => ({
  index: 0,
  label: 'Qwen Code',
  tool: null,
  method: 'npm',
  command: 'npm install -g @qwen-code/qwen-code',
  source: 'https://vendor.example/docs',
  source_host: 'vendor.example',
  high_risk: false,
  note: '',
  ...over,
});

const detailFor = (over: Partial<StoreDetail> = {}): StoreDetail => ({
  id: 'qwen',
  label: 'Qwen Code',
  icon: 'Q',
  tagline: "Alibaba's terminal coding agent.",
  tier: 'starter',
  builtin: false,
  categories: ['open_source'],
  status: 'active',
  replaced_by: '',
  vendor: {
    name: 'Alibaba Cloud (Qwen)',
    homepage: 'https://vendor.example/qwen',
    license: 'Apache-2.0',
    license_kind: 'open_source',
    license_url: 'https://vendor.example/qwen/LICENSE',
    terms_url: null,
  },
  sign_in: { kind: 'account', summary: 'Open it and type /auth, then follow the screen.' },
  checked: { date: '2026-10-28', version: '0.26.2', how: 'package_registry', windows_tested: false },
  state: 'available' as StoreState,
  state_reason: 'Not on this PC yet.',
  installed_version: null,
  modes: ['Pane'],
  can_install: true,
  ambiguous_name: false,
  last_check: null,
  install_plan: { agent_id: 'qwen', label: 'Qwen Code', available: true, reason: null, hint: null, steps: [step()] },
  run_preview: ['qwen', '--acp'],
  fixtures: { count: 0 },
  uninstall: { available: false, steps: [], note: '' },
  found_path_display: null,
  ...over,
});

function fakeStore(detail: StoreDetail, over: Partial<AgentStoreApi> = {}): AgentStoreApi {
  return {
    list: vi.fn(),
    detail: vi.fn(async () => detail),
    add: vi.fn(async () => ({ ok: true, id: 'qwen', state: 'ready' as StoreState })),
    remove: vi.fn(async () => undefined),
    recordInstall: vi.fn(async () => undefined),
    uninstallPlan: vi.fn(),
    uninstallRun: vi.fn(async () => ({ pane_id: 'u1', command: 'x', label: 'Qwen Code' })),
    recordCheck: vi.fn(),
    ...over,
  };
}

const finished: InstallStatus = { known: true, done: true, exit_code: 0, message: null };

function fakeInstall(over: Partial<AgentInstallApi> = {}): AgentInstallApi {
  return {
    plan: vi.fn(),
    run: vi.fn(async () => ({ pane_id: 'p1', command: 'x', label: 'Qwen Code' })),
    status: vi.fn(async () => finished),
    finish: vi.fn(async () => ({
      path_added: [],
      health: {} as never,
      needs_restart: false,
    })),
    ...over,
  };
}

// The finish result must say the agent is installed for the chain to add it.
const installedFinish = (api: AgentInstallApi): AgentInstallApi => ({
  ...api,
  finish: vi.fn(async () => ({
    path_added: [],
    health: { installed: true } as never,
    needs_restart: false,
  })),
});

function mount(props: Partial<Parameters<typeof AgentDetail>[0]> & { store: AgentStoreApi }) {
  const onOpenAgent = vi.fn();
  const onChanged = vi.fn();
  const onClose = vi.fn();
  const openExternal = vi.fn(async () => undefined);
  const { store, ...rest } = props;
  return {
    onOpenAgent,
    onChanged,
    onClose,
    openExternal,
    store,
    render: async (installApi: AgentInstallApi = fakeInstall()) => {
      await act(async () => {
        root.render(
          <AgentDetail
            id="qwen"
            api={store}
            installApi={installApi}
            onOpenAgent={onOpenAgent}
            onChanged={onChanged}
            onClose={onClose}
            openExternal={openExternal}
            {...rest}
          />,
        );
      });
      await flush();
    },
  };
}

describe('AgentDetail facts', () => {
  it('loads the entry through the Store client by default', async () => {
    vi.mocked(invoke).mockResolvedValue(detailFor());
    await act(async () => {
      root.render(<AgentDetail id="qwen" onOpenAgent={vi.fn()} onChanged={vi.fn()} onClose={vi.fn()} />);
    });
    await flush();
    expect(invoke).toHaveBeenCalledWith('agent_store_detail', { id: 'qwen' });
    expect(text()).toContain('Qwen Code');
  });

  it('shows the tier, license, sign-in sentence and the run command', async () => {
    const m = mount({ store: fakeStore(detailFor()) });
    await m.render();
    expect(text()).toContain('Basic support');
    expect(text()).toContain('Open source');
    expect(text()).toContain('Open it and type /auth, then follow the screen.');
    expect(text()).toContain('Puppet Master never sees your password or key.');
    expect(host.querySelector('[data-testid="run-command"]')?.textContent).toBe('qwen --acp');
  });

  it('says we have not run it on Windows when that is not checked', async () => {
    await mount({ store: fakeStore(detailFor()) }).render();
    expect(text()).toContain('We have not run this on Windows ourselves.');
  });

  it('does not say that when the entry was run on Windows', async () => {
    const d = detailFor({ checked: { date: '2026-10-28', version: '0.26.2', how: 'ran_on_windows', windows_tested: true } });
    await mount({ store: fakeStore(d) }).render();
    expect(text()).not.toContain('We have not run this on Windows ourselves.');
  });

  it('shows catalog text as plain text and creates no links from it', async () => {
    const d = detailFor({ tagline: 'Try <img src=x onerror=alert(1)> at https://evil.example now' });
    await mount({ store: fakeStore(d) }).render();
    expect(host.querySelector('img')).toBeNull();
    expect(host.querySelector('a')).toBeNull();
    expect(text()).toContain('<img src=x onerror=alert(1)>');
    expect(text()).toContain('https://evil.example');
  });

  it('shows each link in full and opens it through the external-link helper', async () => {
    const m = mount({ store: fakeStore(detailFor()) });
    await m.render();
    await click(button('https://vendor.example/qwen'));
    expect(m.openExternal).toHaveBeenCalledWith('https://vendor.example/qwen');
    await click(button('https://vendor.example/qwen/LICENSE'));
    expect(m.openExternal).toHaveBeenLastCalledWith('https://vendor.example/qwen/LICENSE');
  });

  it('shows a non-https link as text with no button', async () => {
    const d = detailFor({ vendor: { ...detailFor().vendor, homepage: 'http://vendor.example/qwen', license_url: null } });
    await mount({ store: fakeStore(d) }).render();
    expect(text()).toContain('http://vendor.example/qwen');
    expect(button('http://vendor.example/qwen')).toBeUndefined();
  });
});

describe('AgentDetail install', () => {
  it('shows the exact command in the install sheet before anything runs', async () => {
    const install = fakeInstall();
    const store = fakeStore(detailFor());
    await mount({ store }).render(install);
    await click(button('Install Qwen Code'));
    expect(host.querySelector('[data-testid="install-command"]')?.textContent).toBe('npm install -g @qwen-code/qwen-code');
    expect(text()).toContain('This comes from vendor.example.');
    expect(install.run).not.toHaveBeenCalled();
  });

  it('runs the step, records it and adds the agent after the window ends', async () => {
    const install = installedFinish(fakeInstall());
    const store = fakeStore(detailFor());
    const m = mount({ store });
    await m.render(install);
    await click(button('Install Qwen Code'));
    await click(button('Run'));
    await flush();
    expect(install.run).toHaveBeenCalledWith('qwen', 0, 'npm install -g @qwen-code/qwen-code', false);
    expect(store.recordInstall).toHaveBeenCalledWith('qwen', 0);
    expect(store.add).toHaveBeenCalledWith('qwen', true);
    expect(m.onChanged).toHaveBeenCalled();
  });

  it('keeps Run disabled for a script until the trust box is ticked', async () => {
    const script = step({ method: 'script', high_risk: true, source_host: 'get.example', command: 'irm https://get.example/x | iex' });
    const d = detailFor({ install_plan: { agent_id: 'qwen', label: 'Qwen Code', available: true, reason: null, hint: null, steps: [script] } });
    const install = installedFinish(fakeInstall());
    await mount({ store: fakeStore(d) }).render(install);
    await click(button('Install Qwen Code'));
    expect(button('Run')?.disabled).toBe(true);
    await click(host.querySelector<HTMLInputElement>('input[type="checkbox"]') ?? undefined);
    await click(button('Run'));
    await flush();
    expect(install.run).toHaveBeenCalledWith('qwen', 0, script.command, true);
  });

  it('adds nothing and says so when the install did not finish', async () => {
    const install = fakeInstall({ status: vi.fn(async () => ({ ...finished, exit_code: 1, message: 'Access denied.' })) });
    const store = fakeStore(detailFor());
    await mount({ store }).render(install);
    await click(button('Install Qwen Code'));
    await click(button('Run'));
    await flush();
    expect(text()).toContain('Access denied. Your PC is fine. You can try again.');
    expect(store.add).not.toHaveBeenCalled();
  });

  it('says what the install does in plain words on the card', async () => {
    await mount({ store: fakeStore(detailFor()) }).render();
    expect(text()).toContain('Install runs the command in a window you can watch. Then Puppet Master adds Qwen Code to your list.');
  });
});

describe('AgentDetail add', () => {
  const found = detailFor({ state: 'found_on_pc', can_install: false, found_path_display: '~\\AppData\\qwen.cmd' });

  it('adds a program that is found, with one plain button', async () => {
    const store = fakeStore(found);
    const m = mount({ store });
    await m.render();
    await click(button('Add to Puppet Master'));
    expect(store.add).toHaveBeenCalledWith('qwen', true);
    expect(m.onChanged).toHaveBeenCalled();
  });

  it('asks whether an ambiguous program is the right one before adding it', async () => {
    const store = fakeStore({ ...found, ambiguous_name: true });
    await mount({ store }).render();
    expect(text()).toContain('Is it Qwen Code? Press the button only if it is.');
    expect(text()).toContain('~\\AppData\\qwen.cmd');
    await click(button('Yes, add Qwen Code'));
    expect(store.add).toHaveBeenCalledWith('qwen', true);
  });
});

describe('AgentDetail open, remove and uninstall', () => {
  const ready = detailFor({ state: 'ready', can_install: false, found_path_display: '~\\qwen.cmd' });

  it('opens the agent in its own pane', async () => {
    const m = mount({ store: fakeStore(ready) });
    await m.render();
    await click(button('Open Qwen Code'));
    expect(m.onOpenAgent).toHaveBeenCalledWith('qwen');
  });

  it('removes the Store file only after the confirm', async () => {
    const store = fakeStore(ready);
    const m = mount({ store });
    await m.render();
    await click(button('Remove from Puppet Master'));
    expect(store.remove).not.toHaveBeenCalled();
    expect(text()).toContain('The program stays on this PC.');
    await click(button('Remove from Puppet Master'));
    expect(store.remove).toHaveBeenCalledWith('qwen');
    expect(m.onChanged).toHaveBeenCalled();
  });

  it('shows the uninstall command and runs it only after the confirm', async () => {
    const plan: InstallPlan = {
      agent_id: 'qwen',
      label: 'Qwen Code',
      available: true,
      reason: null,
      hint: null,
      steps: [step({ method: 'npm', command: 'npm uninstall -g @qwen-code/qwen-code' })],
    };
    const store = fakeStore({ ...ready, uninstall: { available: true, steps: plan.steps, note: '' } }, {
      uninstallPlan: vi.fn(async () => plan),
    });
    const install = fakeInstall();
    await mount({ store }).render(install);
    await click(button('Uninstall the program'));
    expect(store.uninstallPlan).toHaveBeenCalledWith('qwen');
    expect(host.querySelector('[data-testid="uninstall-command"]')?.textContent).toBe('npm uninstall -g @qwen-code/qwen-code');
    expect(store.uninstallRun).not.toHaveBeenCalled();
    await click(button('Uninstall Qwen Code'));
    await flush();
    expect(store.uninstallRun).toHaveBeenCalledWith('qwen', 0, 'npm uninstall -g @qwen-code/qwen-code');
    expect(install.finish).toHaveBeenCalledWith('qwen');
  });

  it('shows the vendor removal note for a script install and no uninstall button', async () => {
    const d = { ...ready, uninstall: { available: false, steps: [], note: 'Use the vendor menu to remove it.' } };
    await mount({ store: fakeStore(d) }).render();
    expect(button('Uninstall the program')).toBeUndefined();
    expect(text()).toContain('To remove the program: Use the vendor menu to remove it.');
  });
});

describe('AgentDetail states with no action', () => {
  it('shows the reason and no install or add button for your own definition', async () => {
    const d = detailFor({ state: 'own_version', state_reason: 'You have your own qwen definition.', can_install: false });
    await mount({ store: fakeStore(d) }).render();
    expect(text()).toContain('You have your own qwen definition.');
    expect(button('Install Qwen Code')).toBeUndefined();
    expect(button('Add to Puppet Master')).toBeUndefined();
  });

  it('offers the vendor page when there is no Windows install command', async () => {
    const d = detailFor({ can_install: false, install_plan: { agent_id: 'qwen', label: 'Qwen Code', available: false, reason: 'No install here.', hint: null, steps: [] } });
    await mount({ store: fakeStore(d) }).render();
    expect(text()).toContain("Puppet Master cannot install Qwen Code from here. The vendor's page has the install steps:");
    expect(button('Install Qwen Code')).toBeUndefined();
  });

  it('shows a plain error when the entry cannot be loaded', async () => {
    const store = fakeStore(detailFor(), { detail: vi.fn(async () => { throw 'AGENT_STORE_UNKNOWN: No agent with that id.'; }) });
    await mount({ store }).render();
    expect(text()).toContain('No agent with that id.');
  });
});
