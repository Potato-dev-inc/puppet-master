// @vitest-environment jsdom
// The Artifacts page: the rows and the newest version of each name, the sandboxed HTML frame, the version select, the
// filters, the grid remembered on this computer, Download (through the save dialog), Show in folder, delete with a
// confirm, the unsupported-type note, and "Save as file" under a message.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fakeArtifactTransport, type FakeArtifact } from '../../../lib/potato-artifacts';
import { ArtifactsPage } from './ArtifactsPage';
import { SaveAsFile } from './SaveAsFile';
import { buttonByText } from './test-helpers';

const { save } = vi.hoisted(() => ({ save: vi.fn() }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ save }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  save.mockReset();
  globalThis.localStorage?.clear();
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const settle = () => act(async () => new Promise((r) => setTimeout(r, 0)));

const items = (): FakeArtifact[] => [
  { id: 'a-1', name: 'plan.md', bot: 'nimbus', kind: 'markdown', size: 3, created: 1_000, version: 1, file: 'a-1.md', content: 'first draft' },
  { id: 'a-2', name: 'Plan.md', bot: 'nimbus', kind: 'markdown', size: 3, created: 2_000, version: 2, file: 'a-2.md', content: 'second draft' },
  { id: 'a-3', name: 'page.html', bot: 'nimbus', kind: 'html', size: 9, created: 3_000, version: 1, file: 'a-3.html', content: '<p>hi</p><script>x()</script>' },
  { id: 'a-4', name: 'report.pdf', bot: 'tango', kind: 'pdf', size: 900, created: 500, version: 1, file: 'a-4.pdf' },
];
const bots = [
  { id: 'nimbus', name: 'Nimbus' },
  { id: 'tango', name: 'Tango' },
];

function renderPage(seed = items(), onClose = vi.fn(), onNotify = vi.fn()) {
  const transport = fakeArtifactTransport({ items: seed });
  act(() => {
    root.render(<ArtifactsPage bots={bots} transport={transport} onClose={onClose} onNotify={onNotify} />);
  });
  return { transport, onClose, onNotify };
}

const rows = () => [...host.querySelectorAll<HTMLButtonElement>('.ck-artifact-row')];
const rowFor = (name: string) => rows().find((r) => r.textContent?.includes(name));
const click = (el: Element | null | undefined) => act(() => (el as HTMLElement).click());
const chip = (label: string) => [...host.querySelectorAll<HTMLButtonElement>('.ck-artifacts-chip')].find((b) => b.textContent === label);

function typeIn(el: HTMLInputElement, value: string) {
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(el, value);
  act(() => {
    el.dispatchEvent(new Event('input', { bubbles: true }));
  });
}

function pick(el: HTMLSelectElement, value: string) {
  Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value')!.set!.call(el, value);
  act(() => {
    el.dispatchEvent(new Event('change', { bubbles: true }));
  });
}

describe('the Artifacts page', () => {
  it('lists one row per name, the newest version, and opens the first one', async () => {
    renderPage();
    await settle();
    expect(rows().map((r) => r.querySelector('.ck-artifact-row-name')?.textContent)).toEqual(['page.html', 'Plan.md', 'report.pdf']);
    expect(rowFor('Plan.md')?.textContent).toContain('2 versions');
    expect(rowFor('Plan.md')?.textContent).toContain('Nimbus');
    expect(rowFor('page.html')?.getAttribute('aria-current')).toBe('true');
    expect(host.querySelector('.ck-artifact-view h3')?.textContent).toBe('page.html');
  });

  it('draws an HTML artifact in a frame with no sandbox permissions and no script', async () => {
    renderPage();
    await settle();
    const frame = host.querySelector('iframe')!;
    expect(frame.getAttribute('sandbox')).toBe('');
    expect(frame.getAttribute('sandbox')).not.toMatch(/allow-(scripts|same-origin)/);
    expect(frame.getAttribute('srcdoc')).toContain("default-src 'none'");
    expect(host.textContent).toContain('Preview only. It can\'t reach your account.');
  });

  it('shows text as plain text, never as HTML', async () => {
    renderPage();
    await settle();
    click(rowFor('Plan.md'));
    await settle();
    expect(host.querySelector('pre.ck-artifact-text')?.textContent).toBe('second draft');
    expect(host.querySelector('iframe')).toBeNull();
  });

  it('lets the person open an older version of a name from the Version select', async () => {
    const { transport } = renderPage();
    await settle();
    click(rowFor('Plan.md'));
    await settle();
    const select = host.querySelector<HTMLSelectElement>('.ck-artifact-version select')!;
    expect(select.options.length).toBe(2);
    pick(select, 'a-1');
    await settle();
    expect(host.querySelector('pre.ck-artifact-text')?.textContent).toBe('first draft');
    expect(transport.calls.filter((c) => c.method === 'open').at(-1)?.args).toEqual(['nimbus', 'a-1']);
  });

  it('filters by kind behind the Filters button, and keeps the search trimmed', async () => {
    const { transport } = renderPage();
    await settle();
    expect(host.querySelector('.ck-artifacts-chips')).toBeNull();
    click(buttonByText(host, 'Filters'));
    click(chip('PDF'));
    await settle();
    expect(rows().map((r) => r.querySelector('.ck-artifact-row-name')?.textContent)).toEqual(['report.pdf']);
    expect(transport.calls.filter((c) => c.method === 'list').at(-1)?.args[0]).toEqual({ kind: 'pdf' });
    click(chip('All kinds'));
    click(chip('Tango'));
    await settle();
    expect(transport.calls.filter((c) => c.method === 'list').at(-1)?.args[0]).toEqual({ bot: 'tango' });
    typeIn(host.querySelector<HTMLInputElement>('input.ck-artifacts-search')!, '  nothing  ');
    await settle();
    expect(host.textContent).toContain('No artifacts match these filters.');
  });

  it('says so when there are no artifacts at all', async () => {
    renderPage([]);
    await settle();
    expect(host.textContent).toContain('No artifacts found.');
    expect(host.querySelector('.ck-artifact-view')).toBeNull();
  });

  it('shows a note and the download choices for a PDF, which has no preview', async () => {
    renderPage();
    await settle();
    click(rowFor('report.pdf'));
    await settle();
    expect(host.textContent).toContain("Preview isn't available for this file type. Download it to view it.");
    expect(host.querySelector('iframe')).toBeNull();
    expect(buttonByText(host, 'Download')).toBeDefined();
    expect(buttonByText(host, 'Show in folder')).toBeDefined();
  });

  it('saves a copy through the save dialog, and gives up quietly when the person cancels', async () => {
    save.mockResolvedValueOnce('C:\\Users\\me\\report.pdf');
    const { transport, onNotify } = renderPage();
    await settle();
    click(rowFor('report.pdf'));
    await settle();
    click(buttonByText(host, 'Download'));
    await settle();
    expect(save).toHaveBeenCalledWith({ defaultPath: 'report.pdf' });
    expect(transport.calls.at(-1)).toEqual({ method: 'download', args: ['tango', 'a-4', 'C:\\Users\\me\\report.pdf'] });
    expect(onNotify).toHaveBeenLastCalledWith('Saved a copy of report.pdf.');

    save.mockResolvedValueOnce(null);
    const before = transport.calls.length;
    click(buttonByText(host, 'Download'));
    await settle();
    expect(transport.calls.length).toBe(before);
  });

  it('asks before a delete, deletes one version, and reads the list again', async () => {
    const { transport, onNotify } = renderPage();
    await settle();
    click(rowFor('Plan.md'));
    await settle();
    click(host.querySelector<HTMLButtonElement>('.ck-artifact-text-btn'));
    expect(host.textContent).toContain("Delete version 2 of Plan.md? This can't be undone.");
    expect(transport.calls.some((c) => c.method === 'remove')).toBe(false);
    click(buttonByText(host, 'Cancel'));
    expect(host.querySelector('.ck-artifact-confirm')).toBeNull();

    click(host.querySelector<HTMLButtonElement>('.ck-artifact-text-btn'));
    click(host.querySelector<HTMLButtonElement>('.ck-artifact-confirm .ck-btn--primary'));
    await settle();
    expect(transport.calls.find((c) => c.method === 'remove')?.args).toEqual(['nimbus', 'a-2']);
    expect(onNotify).toHaveBeenLastCalledWith('Deleted Plan.md.');
    expect(rowFor('plan.md')?.textContent).not.toContain('versions');
    expect(host.querySelector('pre.ck-artifact-text')?.textContent).toBe('first draft');
  });

  it('shows a grid when asked, and remembers the choice on this computer', async () => {
    renderPage();
    await settle();
    expect(host.querySelector('.ck-artifacts-list')?.classList.contains('is-list')).toBe(true);
    click(host.querySelector<HTMLButtonElement>('button[aria-label="Show as grid"]'));
    expect(host.querySelector('.ck-artifacts-list')?.classList.contains('is-grid')).toBe(true);
    expect(globalThis.localStorage.getItem('pm.potato.artifacts.view')).toBe('grid');
    expect(host.querySelector('button[aria-label="Show as list"]')).not.toBeNull();
  });

  it('goes back to the chat when asked', async () => {
    const { onClose } = renderPage();
    await settle();
    click(buttonByText(host, 'Back to chat'));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('reports a list that could not be read, in a sentence', async () => {
    const transport = fakeArtifactTransport({ items: [] });
    transport.list = () => Promise.reject(new Error('POTATO_ARTIFACT_INVALID: The artifacts index is damaged.'));
    act(() => {
      root.render(<ArtifactsPage bots={bots} transport={transport} onClose={() => {}} onNotify={() => {}} />);
    });
    await settle();
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('The artifacts index is damaged.');
  });
});

describe('Save as file under a message', () => {
  it('saves the message and says where it went', async () => {
    const files = fakeArtifactTransport();
    act(() => {
      root.render(<SaveAsFile botId="nimbus" messageId="m-7" files={files} />);
    });
    click(buttonByText(host, 'Save as file'));
    await settle();
    expect(files.calls.at(-1)?.method).toBe('saveMessage');
    expect(host.textContent).toBe('Saved as message.md. Find it in Artifacts.');
  });

  it('says why a save failed', async () => {
    const files = fakeArtifactTransport({ fail: { saveMessage: 'The disk is full.' } });
    act(() => {
      root.render(<SaveAsFile botId="nimbus" messageId="m-7" files={files} />);
    });
    click(buttonByText(host, 'Save as file'));
    await settle();
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('The disk is full.');
    expect(buttonByText(host, 'Save as file')).toBeDefined();
  });
});
