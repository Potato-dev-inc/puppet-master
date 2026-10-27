// @vitest-environment jsdom
import { act, type ReactNode } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ChangesCard } from './ChangesCard';
import { ContextCard } from './ContextCard';
import { HelpersCard } from './HelpersCard';
import { TasksCard } from './TasksCard';
import { WhereCard } from './WhereCard';

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
});
const show = (node: ReactNode) => act(() => root.render(node));
const usage = (inputTokens: number, outputTokens: number, contextWindow: number | null) => ({
  kind: 'usage' as const,
  id: 'u',
  at: null,
  inputTokens,
  outputTokens,
  contextWindow,
});

describe('WhereCard', () => {
  it('shows agent, model, version, folder, branch and account', () => {
    show(<WhereCard agentName="Claude Code" model="opus-x" cliVersion="2.1.0" folder="/work/shop/app" branch="main" accountLabel="Work" />);
    const text = host.textContent ?? '';
    expect(text).toContain('Claude Code v2.1.0');
    expect(text).toContain('opus-x');
    expect(text).toContain('shop/app');
    expect(text).toContain('main');
    expect(text).toContain('Work');
    expect(host.querySelector('[title="/work/shop/app"]')).not.toBeNull();
  });
  it('leaves out what is not known', () => {
    show(<WhereCard agentName="Helper" model={null} cliVersion={null} folder="/work/shop" branch={null} />);
    const text = host.textContent ?? '';
    expect(text).not.toContain('Model');
    expect(text).not.toContain('Branch');
    expect(text).not.toContain('Account');
  });
});

describe('ContextCard', () => {
  it('draws the bar and the sentence', () => {
    show(<ContextCard usage={usage(47_000, 1000, 200_000)} />);
    expect(host.textContent).toContain('48k of 200k tokens (24%)');
    expect(host.querySelector('[role="meter"]')?.getAttribute('aria-valuenow')).toBe('24');
    expect(host.querySelector('[role="meter"]')?.getAttribute('title')).toBe('Input 47,000 tokens, output 1,000 tokens');
  });
  it('says so when the agent does not report it', () => {
    show(<ContextCard usage={usage(5000, 10, null)} />);
    expect(host.textContent).toContain('Not reported by this agent');
    expect(host.querySelector('[role="meter"]')).toBeNull();
    show(<ContextCard usage={null} />);
    expect(host.textContent).toContain('Not reported by this agent');
  });
});

describe('TasksCard', () => {
  it('counts done tasks and marks each state', () => {
    show(
      <TasksCard
        plan={[
          { text: 'Read the code', status: 'completed' },
          { text: 'Write the fix', status: 'in_progress' },
          { text: 'Run the tests', status: 'pending' },
        ]}
      />,
    );
    expect(host.textContent).toContain('1 of 3 done');
    expect(Array.from(host.querySelectorAll('[role="img"]')).map((d) => d.getAttribute('aria-label'))).toEqual(['Done', 'Working on it', 'To do']);
  });
  it('is hidden with no plan', () => {
    show(<TasksCard plan={[]} />);
    expect(host.innerHTML).toBe('');
  });
});

describe('ChangesCard', () => {
  const outputs = [
    { path: '/work/shop/src/a.ts', tool: 'Edit', wrote: true },
    { path: '/work/shop/README.md', tool: 'Read', wrote: false },
    { path: '/work/shop/notes.txt', tool: 'Write', wrote: true },
  ];
  it('lists only written files with name and folder', () => {
    show(<ChangesCard outputs={outputs} cwd="/work/shop" />);
    expect(host.textContent).toContain('+2 files');
    expect(host.textContent).toContain('a.ts');
    expect(host.textContent).toContain('src');
    expect(host.textContent).toContain('notes.txt');
    expect(host.textContent).not.toContain('README');
    expect(host.querySelector('button')).toBeNull();
  });
  it('opens the Changes tab on a click when there is one', () => {
    const open = vi.fn();
    show(<ChangesCard outputs={outputs} cwd="/work/shop" onOpenChanges={open} />);
    act(() => host.querySelector<HTMLButtonElement>('button')!.click());
    expect(open).toHaveBeenCalledTimes(1);
  });
  it('is hidden when nothing was written', () => {
    show(<ChangesCard outputs={[outputs[1]]} cwd="/work/shop" />);
    expect(host.innerHTML).toBe('');
  });
});

describe('HelpersCard', () => {
  it('shows each helper and opens it', () => {
    const open = vi.fn();
    show(
      <HelpersCard
        helpers={[
          { id: 'h1', title: 'Check tests', agent: 'Codex', status: 'running' },
          { id: 'h2', title: 'Claude Code', agent: null, status: 'idle' },
        ]}
        onOpen={open}
      />,
    );
    const rows = host.querySelectorAll('button');
    expect(rows).toHaveLength(2);
    expect(rows[0].textContent).toContain('Check tests');
    expect(rows[0].textContent).toContain('Codex');
    act(() => rows[1].click());
    expect(open).toHaveBeenCalledWith('h2');
  });
  it('is hidden with no helpers', () => {
    show(<HelpersCard helpers={[]} onOpen={() => {}} />);
    expect(host.innerHTML).toBe('');
  });
});
