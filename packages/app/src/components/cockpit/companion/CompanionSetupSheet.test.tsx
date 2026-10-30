// @vitest-environment jsdom
// The five steps of "Make your companion": an agent that cannot take messages is shown with its reason and cannot be
// picked; the button needs a name, a folder and a usable agent; a face sets the default name until the person types.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fakeCompanionTransport, type CompanionAgentOption } from '../../../lib/companion';
import type { ShipApi } from '../../../lib/ship-api';
import { CompanionSetupSheet } from './CompanionSetupSheet';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

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
  vi.restoreAllMocks();
});

const AGENTS: CompanionAgentOption[] = [
  { id: 'claude', label: 'Claude Code', ok: true, reason: null },
  { id: 'codex', label: 'Codex', ok: false, reason: "Chat can't send messages to this agent yet." },
];

const button = (label: string) => [...host.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.trim().startsWith(label))!;
const nameInput = () => host.querySelector<HTMLInputElement>('input.ck-companion-input')!;

function setValue(el: HTMLInputElement, value: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!;
  act(() => {
    setter.call(el, value);
    el.dispatchEvent(new Event('input', { bubbles: true }));
  });
}

async function render(agents: CompanionAgentOption[] = AGENTS, ship: Pick<ShipApi, 'pickFolder'> = { pickFolder: vi.fn(async () => null) }) {
  const transport = fakeCompanionTransport({ agents });
  const onCreated = vi.fn();
  const onClose = vi.fn();
  await act(async () => root.render(<CompanionSetupSheet transport={transport} ship={ship} onClose={onClose} onCreated={onCreated} />));
  await act(async () => {});
  return { transport, onCreated, onClose, ship };
}

describe('CompanionSetupSheet', () => {
  it('reads the agents, shows the reason for one that cannot take messages, and picks a usable one', async () => {
    await render();
    expect(button('Codex').disabled).toBe(true);
    expect(host.textContent).toContain("Chat can't send messages to this agent yet.");
    expect(button('Claude Code').getAttribute('aria-checked')).toBe('true');
  });

  it('names the face by default, and keeps the typed name when the face changes', async () => {
    await render();
    expect(nameInput().value).toBe('Mochi');
    act(() => host.querySelector<HTMLButtonElement>('button[title="Bit"]')!.click());
    expect(nameInput().value).toBe('Bit');
    setValue(nameInput(), 'Pip');
    act(() => host.querySelector<HTMLButtonElement>('button[title="Nib"]')!.click());
    expect(nameInput().value).toBe('Pip');
  });

  it('makes the companion with the chosen face, name, agent, talk and folder', async () => {
    const { transport, onCreated } = await render();
    act(() => host.querySelector<HTMLButtonElement>('button[title="Lumen"]')!.click());
    setValue(nameInput(), 'Lumi');
    act(() => button('Plain and direct').click());
    expect(button('Wake up Lumi').disabled).toBe(false);
    await act(async () => button('Wake up Lumi').click());
    const create = transport.calls.find((c) => c.method === 'create');
    expect(create?.args[0]).toEqual({
      name: 'Lumi',
      avatar: 'lumen',
      persona: 'plain',
      agentType: 'claude',
      folder: '',
      autoStart: true,
      memoryReview: true,
      napAt: { h: 4, m: 0 },
      checkins: [],
    });
    expect(onCreated).toHaveBeenCalledTimes(1);
  });

  it('does not list plain shells, but keeps other unusable agents greyed with their reason', async () => {
    await render([
      { id: 'claude', label: 'Claude Code', ok: true, reason: null },
      { id: 'codex', label: 'Codex', ok: false, reason: "Chat can't send messages to this agent yet." },
      { id: 'powershell', label: 'PowerShell', ok: false, reason: "Chat can't send messages to this agent yet." },
      { id: 'bash', label: 'Bash', ok: false, reason: "Chat can't send messages to this agent yet." },
    ]);
    expect(host.textContent).not.toContain('PowerShell');
    expect(host.textContent).not.toContain('Bash');
    expect(button('Codex').disabled).toBe(true);
  });

  it('draws each face about 40 px wide', async () => {
    await render();
    expect(host.querySelector('button[title="Bit"] svg')?.getAttribute('width')).toBe('40');
  });

  it('will not make a companion with an empty name', async () => {
    await render();
    setValue(nameInput(), '   ');
    expect(button('Wake up it').disabled).toBe(true);
  });

  it('blocks the button when no agent can take messages', async () => {
    await render([{ id: 'codex', label: 'Codex', ok: false, reason: "Chat can't send messages to this agent yet." }]);
    expect(button('Wake up Mochi').disabled).toBe(true);
  });

  it('starts with a folder of its own that Puppet Master makes, not the open project', async () => {
    await render();
    expect(host.querySelector('.ck-companion-folder-name')?.textContent).toBe('A folder of its own, made for it');
    expect(host.textContent).toContain('a folder of its own that Puppet Master makes');
  });

  it('a folder the person picks holds a folder of its own for the companion, never the picked folder itself', async () => {
    const ship = { pickFolder: vi.fn(async () => ({ path: 'D:\\notes', name: 'notes', state: 'folder' as const, last_used_ms: 0 })) };
    const { transport } = await render(AGENTS, ship as unknown as Pick<ShipApi, 'pickFolder'>);
    await act(async () => button('Change folder').click());
    expect(host.querySelector('.ck-companion-folder-name')?.textContent).toBe('D:\\notes\\mochi');
    expect(host.textContent).toContain('Its files stay on this computer, in D:\\notes\\mochi.');
    // It follows the name.
    setValue(nameInput(), 'Pipi');
    expect(host.querySelector('.ck-companion-folder-name')?.textContent).toBe('D:\\notes\\pipi');
    await act(async () => button('Wake up Pipi').click());
    const create = transport.calls.find((c) => c.method === 'create');
    expect((create?.args[0] as { folder: string }).folder).toBe('D:\\notes\\pipi');
  });

  it('shows the refusal of a broad folder as a plain sentence, without the code', async () => {
    const { transport } = await render();
    transport.create = () =>
      Promise.reject(new Error("COMPANION_FOLDER_BROAD: Pick a folder of its own. Your Desktop has other projects in it, and agents there would read Mochi's notes. Try C:\\x\\mochi."));
    await act(async () => button('Wake up Mochi').click());
    const alert = host.querySelector('[role="alert"]')!.textContent!;
    expect(alert).toBe("Pick a folder of its own. Your Desktop has other projects in it, and agents there would read Mochi's notes. Try C:\\x\\mochi.");
    expect(button('Wake up Mochi').disabled).toBe(false);
  });
});
