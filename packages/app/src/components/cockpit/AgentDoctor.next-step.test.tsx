// @vitest-environment jsdom
// Set up agents gives each row one next step, groups rows by need, and keeps raw codes out of sight.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AgentHealth, AgentHealthApi } from '../../lib/agent-health';
import { AgentDoctor, moreActions, primaryAction } from './AgentDoctor';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

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

const row = (over: Partial<AgentHealth>): AgentHealth => ({
  id: 'a',
  label: 'Agent A',
  state: 'unchecked',
  installed: true,
  version: null,
  reason: 'Installed. Press Test to check it can answer.',
  install_hint: null,
  login_hint: null,
  can_test: true,
  ...over,
});

async function mount(rows: AgentHealth[]) {
  const api: AgentHealthApi = { check: vi.fn(async () => rows), test: vi.fn() };
  await act(async () => {
    root.render(<AgentDoctor api={api} onOpenAgent={() => {}} />);
  });
  await act(async () => { await Promise.resolve(); await Promise.resolve(); });
}
const primaries = (scope: ParentNode) => Array.from(scope.querySelectorAll('.ck-doctor-actions > .ck-btn--primary')).map((b) => b.textContent);

describe('one next step per row', () => {
  it('picks Install, Sign in or Test by state, and nothing for a ready agent', () => {
    expect(primaryAction(row({ state: 'not_installed', installed: false, can_test: false }))).toBe('install');
    expect(primaryAction(row({ state: 'needs_login' }))).toBe('signin');
    expect(primaryAction(row({ state: 'unchecked' }))).toBe('test');
    expect(primaryAction(row({ state: 'error' }))).toBe('test');
    expect(primaryAction(row({ state: 'ready' }))).toBeNull();
  });

  it('keeps the other actions out of the way', () => {
    const missing = row({ state: 'not_installed', installed: false, can_test: false, install_hint: 'x' });
    expect(moreActions(missing, primaryAction(missing))).toEqual(['copy']);
    const unchecked = row({ state: 'unchecked' });
    expect(moreActions(unchecked, primaryAction(unchecked))).toEqual(['signin']);
    const ready = row({ state: 'ready' });
    expect(moreActions(ready, primaryAction(ready))).toEqual(['signin', 'test']);
  });

  it('shows at most one primary button per row', async () => {
    await mount([
      row({ id: 'a', state: 'unchecked' }),
      row({ id: 'b', state: 'needs_login' }),
      row({ id: 'c', state: 'not_installed', installed: false, can_test: false, install_hint: 'npm i -g c' }),
      row({ id: 'd', state: 'ready' }),
    ]);
    for (const li of Array.from(host.querySelectorAll('.ck-doctor-row'))) {
      expect(li.querySelectorAll('.ck-doctor-actions > .ck-btn--primary').length).toBeLessThanOrEqual(1);
    }
    expect(primaries(host)).toEqual(['Test', 'Sign in', 'Install']);
  });
});

describe('grouped by need', () => {
  it('lists what needs you first, then Ready, then Not installed', async () => {
    await mount([
      row({ id: 'r', label: 'Ready one', state: 'ready' }),
      row({ id: 'm', label: 'Missing one', state: 'not_installed', installed: false, can_test: false }),
      row({ id: 'n', label: 'Needs one', state: 'needs_login' }),
    ]);
    const caps = Array.from(host.querySelectorAll('.ck-doctor-cap')).map((e) => e.textContent);
    expect(caps).toEqual(['Needs you', 'Ready', 'Not installed']);
    const order = Array.from(host.querySelectorAll('.ck-row-title')).map((e) => e.textContent);
    expect(order).toEqual(['Needs one', 'Ready one', 'Missing one']);
  });
});

describe('plain errors', () => {
  it('turns "exit code 1" into a sentence with a next step, and keeps the code in the tooltip', async () => {
    const reason = 'Found it, but it did not start properly: exit code 1';
    await mount([row({ label: 'Cursor Agent', state: 'error', reason })]);
    const p = host.querySelector('.ck-doctor-reason') as HTMLElement;
    expect(p.textContent).toBe('Cursor Agent did not start. Try Test again, or reinstall it.');
    expect(p.textContent).not.toContain('exit code');
    expect(p.title).toBe(reason);
  });

  it('puts the login command and the install command under Details, never in the main line', async () => {
    await mount([
      row({ id: 'n', state: 'needs_login', login_hint: 'Run: codex login' }),
      row({ id: 'm', state: 'not_installed', installed: false, can_test: false, install_hint: 'npm i -g x' }),
    ]);
    for (const el of Array.from(host.querySelectorAll('.ck-doctor-reason'))) {
      expect(el.textContent).not.toContain('codex login');
      expect(el.textContent).not.toContain('npm i -g');
    }
    const details = Array.from(host.querySelectorAll('.ck-doctor-details')).map((d) => d.textContent);
    expect(details.some((t) => t?.includes('Run: codex login'))).toBe(true);
    expect(details.some((t) => t?.includes('npm i -g x'))).toBe(true);
  });
});
