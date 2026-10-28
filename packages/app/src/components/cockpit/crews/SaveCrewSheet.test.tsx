// @vitest-environment jsdom
// "Save this team": the draft is shown for editing, an edit reaches the saved crew, a role can be left out, a server problem is
// shown on its role, and Save is blocked while a task is empty.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Crew, CrewsApi } from '../../../lib/crews-api';
import { SaveCrewSheet } from './SaveCrewSheet';

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

const draft = (): Crew => ({
  name: 'Lead team',
  roles: [
    { name: 'Lead', agent_type: 'alpha', task: 'Plan the review of {folder}' },
    { name: 'Checker', agent_type: 'wizard_made', task: 'Run the checks in {folder}', parent: 'Lead' },
  ],
});

const names: Record<string, string> = { alpha: 'Alpha', wizard_made: 'Wizard Made' };

function api(over: Partial<CrewsApi> = {}, omitted = 0): CrewsApi {
  return {
    list: vi.fn(async () => []),
    draft: vi.fn(async () => ({ crew: draft(), omitted })),
    save: vi.fn(async () => ({ slug: 'lead-team', errors: [] })),
    remove: vi.fn(async () => {}),
    start: vi.fn(),
    ...over,
  } as CrewsApi;
}

async function render(a: CrewsApi) {
  const onClose = vi.fn();
  const onSaved = vi.fn();
  await act(async () => root.render(<SaveCrewSheet paneId="p1" api={a} agentName={(id) => names[id] ?? id} onClose={onClose} onSaved={onSaved} />));
  await act(async () => {});
  return { onClose, onSaved };
}

function setValue(el: HTMLInputElement | HTMLTextAreaElement, value: string) {
  const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  act(() => {
    Object.getOwnPropertyDescriptor(proto, 'value')!.set!.call(el, value);
    el.dispatchEvent(new Event('input', { bubbles: true }));
  });
}

const button = (label: string) => [...host.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.trim() === label)!;
const field = (label: string) => host.querySelector<HTMLInputElement & HTMLTextAreaElement>(`[aria-label="${label}"]`)!;

describe('SaveCrewSheet', () => {
  it('asks the pane for its draft and shows each role with its agent and place', async () => {
    const a = api();
    await render(a);
    expect(a.draft).toHaveBeenCalledWith('p1');
    expect(host.querySelectorAll('[data-testid="crew-role"]')).toHaveLength(2);
    expect(host.textContent).toContain('Alpha · lead');
    expect(host.textContent).toContain('Wizard Made · under Lead');
    expect(field('Task of Checker').value).toBe('Run the checks in {folder}');
  });

  it('saves the edited names and tasks, then reports the crew name', async () => {
    const a = api();
    const { onSaved } = await render(a);
    setValue(field('Crew name'), 'Night review');
    setValue(field('Name of role 2'), 'Tester');
    setValue(field('Task of Tester'), 'Test {folder} twice');
    await act(async () => button('Save crew').click());
    const saved = (a.save as ReturnType<typeof vi.fn>).mock.calls[0][0] as Crew;
    expect(saved.name).toBe('Night review');
    expect(saved.roles[1]).toMatchObject({ name: 'Tester', task: 'Test {folder} twice', parent: 'Lead' });
    expect(onSaved).toHaveBeenCalledWith('Night review');
  });

  it('can leave a helper out but never the lead', async () => {
    const a = api();
    await render(a);
    expect(host.querySelectorAll('button[title^="Leave"]')).toHaveLength(1);
    act(() => button('Leave out').click());
    expect(host.querySelectorAll('[data-testid="crew-role"]')).toHaveLength(1);
    await act(async () => button('Save crew').click());
    expect(((a.save as ReturnType<typeof vi.fn>).mock.calls[0][0] as Crew).roles).toHaveLength(1);
  });

  it('blocks Save while a task is empty and says why', async () => {
    await render(api());
    setValue(field('Task of Lead'), '  ');
    expect(button('Save crew').disabled).toBe(true);
    expect(host.textContent).toContain('Write a task for Lead.');
  });

  it('shows a refusal on the role it belongs to and stays open', async () => {
    const a = api({
      save: vi.fn(async () => ({ slug: null, errors: [{ code: 'CREW_AGENT_UNKNOWN', role: 'Checker', message: 'The agent "wizard_made" for Checker is not set up here.' }] })),
    });
    const { onSaved } = await render(a);
    await act(async () => button('Save crew').click());
    expect(onSaved).not.toHaveBeenCalled();
    const second = host.querySelectorAll('[data-testid="crew-role"]')[1];
    expect(second.textContent).toContain('is not set up here');
    expect(button('Save crew').disabled).toBe(false);
  });

  it('says how many helpers did not fit', async () => {
    await render(api({}, 3));
    expect(host.querySelector('[data-testid="crew-omitted"]')?.textContent).toBe('3 more helpers were left out. A crew holds at most 8 agents.');
  });

  it('shows the error when the pane cannot be read', async () => {
    await render(api({ draft: vi.fn(async () => Promise.reject(new Error("CREW_PANE_GONE: That agent's pane is closed."))) }));
    expect(host.querySelector('[role="alert"]')?.textContent).toBe("That agent's pane is closed.");
    expect(button('Save crew').disabled).toBe(true);
  });
});
