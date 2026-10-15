// @vitest-environment jsdom
// "Make something": three taps, then she reads the words, then Start. Nothing is sent before Start.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { MakeSomething } from './MakeSomething';
import { T } from './copy';
import type { SimpleApi } from './simple-api';

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

const button = (text: string) => Array.from(host.querySelectorAll('button')).find((b) => b.textContent?.includes(text)) as HTMLButtonElement | undefined;
const tap = async (text: string) => {
  const el = button(text);
  expect(el, `a button "${text}"`).toBeTruthy();
  await act(async () => el!.click());
};

function api(over: Partial<SimpleApi> = {}): Pick<SimpleApi, 'start' | 'checkFolder' | 'pickFolder'> {
  return {
    start: vi.fn(async () => ({ pane_id: 'p1', handle: 'h', folder: 'C:\\x' })),
    checkFolder: vi.fn(async () => ({ ok: true as const })),
    pickFolder: vi.fn(async () => null),
    ...over,
  };
}

async function mount(a: ReturnType<typeof api>, onStarted = vi.fn()) {
  await act(async () => {
    root.render(<MakeSomething api={a} onStarted={onStarted} onCancel={() => {}} onOpenSettings={() => {}} />);
  });
  return { onStarted };
}

async function throughThreeQuestions() {
  await tap('A small tool');
  await tap('A class timer');
  await tap('My students or customers');
  await tap('Bright and fun');
}

describe('Make something', () => {
  it('asks 3 tap questions, then shows the words before anything is sent', async () => {
    const a = api();
    await mount(a);
    await tap('A small tool');
    expect(host.textContent).toContain('Question 1 of 3');
    await tap('A class timer');
    expect(host.textContent).toContain('Question 2 of 3');
    await tap('My students or customers');
    expect(host.textContent).toContain('Question 3 of 3');
    await tap('Bright and fun');
    const box = host.querySelector('textarea') as HTMLTextAreaElement;
    expect(box.value).toBe('Make a class timer for my students or customers. It should look bright and fun. Make it as ONE file called tool.html.');
    expect(host.textContent).toContain('Here is what Puppet Master will ask Claude Code.');
    expect(host.textContent).toContain(T.copyNote);
    expect(a.start).not.toHaveBeenCalled();
  });

  it('Start sends the words with the house rules, and no way to skip the copy', async () => {
    const a = api();
    const { onStarted } = await mount(a);
    await throughThreeQuestions();
    await tap(T.makeStart);
    expect(a.start).toHaveBeenCalledTimes(1);
    const input = (a.start as ReturnType<typeof vi.fn>).mock.calls[0][0];
    expect(input.agent).toBe('claude');
    expect(input.name).toBe('Class timer');
    expect(input.prompt).toMatch(/^Make a class timer/);
    expect(input.prompt).toMatch(/Do not delete or move any files/);
    expect(Object.keys(input).sort()).toEqual(['agent', 'name', 'prompt']);
    expect(onStarted).toHaveBeenCalledWith({ pane_id: 'p1', handle: 'h', folder: 'C:\\x' });
  });

  it('she can change the words before Start', async () => {
    const a = api();
    await mount(a);
    await throughThreeQuestions();
    const box = host.querySelector('textarea') as HTMLTextAreaElement;
    await act(async () => {
      const set = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!;
      set.call(box, 'Make a timer with a big red button.');
      box.dispatchEvent(new Event('input', { bubbles: true }));
    });
    await tap(T.makeStart);
    expect((a.start as ReturnType<typeof vi.fn>).mock.calls[0][0].prompt).toMatch(/^Make a timer with a big red button\. Make it as one file/);
  });

  it('a broad folder is refused in plain words and never reaches Start', async () => {
    const a = api({
      pickFolder: vi.fn(async () => ({ path: 'C:\\Users\\Ana\\Documents', name: 'Documents' })),
      checkFolder: vi.fn(async () => ({ ok: false as const, code: 'FOLDER_TOO_BROAD', message: 'documents' })),
    });
    await mount(a);
    await throughThreeQuestions();
    await tap(T.makeFolder);
    await act(async () => {});
    expect(host.textContent).toContain(T.makeFolderTooBroad);
    expect(host.textContent).not.toContain('FOLDER_TOO_BROAD');
    // The refused folder is not kept: Start makes a new project.
    await tap(T.makeStart);
    expect((a.start as ReturnType<typeof vi.fn>).mock.calls[0][0].existing_folder).toBeUndefined();
  });

  it('a good folder is sent as the folder to copy', async () => {
    const a = api({ pickFolder: vi.fn(async () => ({ path: 'D:\\Work\\Orders', name: 'Orders' })) });
    await mount(a);
    await throughThreeQuestions();
    await tap(T.makeFolder);
    await act(async () => {});
    expect(host.textContent).toContain(T.makeFolderOk);
    await tap(T.makeStart);
    const input = (a.start as ReturnType<typeof vi.fn>).mock.calls[0][0];
    expect(input.existing_folder).toBe('D:\\Work\\Orders');
    expect(input.name).toBe('Orders');
  });

  it('a refusal from Rust while starting says the files are safe', async () => {
    const a = api({
      start: vi.fn(async () => {
        throw new Error('FOLDER_NEEDS_SETUP: PM cannot copy this folder yet.');
      }),
    });
    await mount(a);
    await throughThreeQuestions();
    await tap(T.makeStart);
    await act(async () => {});
    expect(host.textContent).toContain(T.makeFolderNeedsSetup);
    expect(host.textContent).not.toContain('FOLDER_NEEDS_SETUP');
  });
});
