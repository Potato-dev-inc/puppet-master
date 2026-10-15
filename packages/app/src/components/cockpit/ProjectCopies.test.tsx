// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PresetView, ShipApi } from '../../lib/ship-api';
import { ProjectCopies } from './ProjectCopies';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const VIEW: PresetView = { setup: ['npm ci'], copy: ['.env', '.env.local'], run: 'npm run dev', conductor: null, problem: null };

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

const button = (text: string) => Array.from(host.querySelectorAll('button')).find((b) => b.textContent?.includes(text)) as HTMLButtonElement | undefined;
const field = (label: string) => host.querySelector(`[aria-label="${label}"]`) as HTMLTextAreaElement | HTMLInputElement;
async function type(el: HTMLTextAreaElement | HTMLInputElement, value: string) {
  await act(async () => {
    const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, 'value')?.set?.call(el, value);
    el.dispatchEvent(new Event('input', { bubbles: true }));
  });
}

async function mount(view: PresetView, over: Partial<ShipApi> = {}, project: string | null = 'D:/code/shop') {
  const ship = {
    presetGet: vi.fn().mockResolvedValue(view),
    presetSet: vi.fn().mockImplementation(async (_p, setup, copy, run) => ({ ...view, setup, copy, run })),
    presetImportConductor: vi.fn().mockResolvedValue({ ...view, setup: ['npm ci', 'npm run build'], run: 'npm run dev' }),
    ...over,
  } as unknown as ShipApi;
  await act(async () => {
    root.render(<ProjectCopies project={project ?? undefined} ship={ship} />);
  });
  return ship;
}

describe('Project > Copies', () => {
  it('shows the setup, the files that are copied and the start command', async () => {
    await mount(VIEW);
    expect(host.textContent).toContain('When Puppet Master makes a separate copy');
    expect(field('Set up with').value).toBe('npm ci');
    expect(field('Copy these files').value).toBe('.env\n.env.local');
    expect(field('Start with').value).toBe('npm run dev');
    expect(host.textContent).toContain('Copied, never committed.');
    expect(button('Save')?.disabled).toBe(true);
  });

  it('saves one command per line and the files as a list', async () => {
    const ship = await mount(VIEW);
    await type(field('Set up with'), 'npm ci\nnpm run build\n');
    await type(field('Copy these files'), '.env, secrets/key.txt');
    expect(button('Save')?.disabled).toBe(false);
    await act(async () => button('Save')?.click());
    expect(ship.presetSet).toHaveBeenCalledWith('D:/code/shop', ['npm ci', 'npm run build'], ['.env', 'secrets/key.txt'], 'npm run dev');
    expect(host.textContent).toContain('Saved. The next separate copy uses it.');
  });

  it('shows the reason when a command cannot be saved', async () => {
    const ship = await mount(VIEW, { presetSet: vi.fn().mockRejectedValue(new Error('INVALID_ARGUMENT: "npm ci && npm test": "&" is not supported here')) });
    await type(field('Set up with'), 'npm ci && npm test');
    await act(async () => button('Save')?.click());
    expect(host.textContent).toContain('is not supported here');
    expect(ship.presetSet).toHaveBeenCalled();
  });

  it('offers to use conductor.json and imports it', async () => {
    const ship = await mount({ ...VIEW, setup: [], run: '', conductor: { setup: ['npm ci', 'npm run build'], run: 'npm run dev', unsupported: null } });
    expect(host.textContent).toContain('This project already has setup steps (conductor.json). Use them?');
    await act(async () => button('Use its setup')?.click());
    expect(ship.presetImportConductor).toHaveBeenCalledWith('D:/code/shop');
    expect(field('Set up with').value).toBe('npm ci\nnpm run build');
  });

  it('explains a conductor.json that cannot be imported', async () => {
    await mount({ ...VIEW, conductor: { setup: [], run: '', unsupported: 'conductor.json setup: "|" is not supported here' } });
    expect(host.textContent).toContain("can't be imported");
    expect(button('Use its setup')).toBeUndefined();
  });

  it('asks for a project first when there is none', async () => {
    await mount(VIEW, {}, null);
    expect(host.textContent).toContain('Open a project first');
  });
});
