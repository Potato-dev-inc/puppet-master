// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { AnnotatedDiff } from './AnnotatedDiff';
import { splitPatch } from './diff';
import { anchorFindings, type ReviewFinding } from './review-diff';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const PATCH = [
  'diff --git a/a.txt b/a.txt',
  'index 111..222 100644',
  '--- a/a.txt',
  '+++ b/a.txt',
  '@@ -1,2 +1,3 @@',
  ' alpha', // 1
  '-bravo',
  '+bravo two', // 2
  '+charlie', // 3
  '',
].join('\n');

const LINES = splitPatch(PATCH).get('a.txt') ?? [];

const FINDING: ReviewFinding = {
  id: 'f1',
  file: 'a.txt',
  line: 2,
  severity: 'high',
  text: 'Missing null check',
};
const HEADER: ReviewFinding = { id: 'f2', file: 'a.txt', line: null, severity: 'info', text: 'File-level note' };

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

async function mount(props: Partial<Parameters<typeof AnnotatedDiff>[0]> = {}) {
  const anchored = anchorFindings(props.findings ?? [], new Map([['a.txt', LINES]])).byFile.get('a.txt') ?? [];
  await act(async () => {
    root.render(
      <AnnotatedDiff
        file="a.txt"
        lines={LINES}
        agentLabel="Claude Code"
        {...props}
        findings={anchored}
      />,
    );
  });
}

const click = async (el: Element | null) => {
  await act(async () => {
    (el as HTMLElement).dispatchEvent(new MouseEvent('click', { bubbles: true }));
  });
};

const rows = () => Array.from(host.querySelectorAll('.ck-diff-line'));

describe('AnnotatedDiff', () => {
  it('shows a finding directly after the line it points at', async () => {
    await mount({ findings: [FINDING] });
    const lineEl = rows().find((r) => r.textContent?.includes('+bravo two'));
    const note = host.querySelector('.ck-find');
    expect(note?.textContent).toContain('Missing null check');
    expect(lineEl?.nextElementSibling).toBe(note);
  });

  it('marks the line with the highest severity glyph in the gutter', async () => {
    await mount({ findings: [FINDING] });
    const mark = host.querySelector('.ck-gutter--mark');
    expect(mark?.textContent).toBe('!!');
    expect(mark?.closest('.ck-diff-line')?.textContent).toContain('+bravo two');
    expect(host.querySelectorAll('.ck-gutter:not(.ck-gutter--mark)')).toHaveLength(rows().length - 1);
  });

  it('renders file-level findings once, above the first line', async () => {
    await mount({ findings: [HEADER] });
    expect(host.querySelectorAll('.ck-find')).toHaveLength(1);
    expect(host.querySelector('pre')?.firstElementChild?.classList.contains('ck-find')).toBe(true);
  });

  it('sends a finding to the original agent with its file, line and text', async () => {
    const onSend = vi.fn().mockResolvedValue(undefined);
    await mount({ findings: [FINDING], onSend });
    const btn = Array.from(host.querySelectorAll('button')).find((b) => b.textContent === 'Send to Claude Code');
    await click(btn ?? null);
    expect(onSend).toHaveBeenCalledWith('a.txt', 2, 'Missing null check');
    expect(host.querySelector('.ck-find-send')?.textContent).toBe('Sent');
    expect((host.querySelector('.ck-find-send') as HTMLButtonElement).disabled).toBe(true);
  });

  it('shows the error from a rejected send in an alert', async () => {
    const onSend = vi.fn().mockRejectedValue(new Error('pane closed'));
    await mount({ findings: [FINDING], onSend });
    await click(host.querySelector('.ck-find-send'));
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('pane closed');
  });

  it('a line comment opens a composer and sends the typed text on that line', async () => {
    const onSend = vi.fn().mockResolvedValue(undefined);
    await mount({ onSend });
    await click(host.querySelector('[aria-label="Comment on line 3"]'));
    const area = host.querySelector('textarea[aria-label="Comment"]') as HTMLTextAreaElement | null;
    expect(area).not.toBeNull();
    await act(async () => {
      const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')?.set;
      setter?.call(area, '  Why not guard this?  ');
      area?.dispatchEvent(new Event('input', { bubbles: true }));
    });
    const send = Array.from(host.querySelectorAll('button')).find((b) => b.textContent === 'Send to Claude Code');
    await click(send ?? null);
    expect(onSend).toHaveBeenCalledWith('a.txt', 3, 'Why not guard this?');
    expect(host.querySelector('textarea[aria-label="Comment"]')).toBeNull();
    expect(host.querySelector('[role="status"]')?.textContent).toBe('Sent');
  });

  it('the comment send button is disabled while the draft is empty', async () => {
    await mount({ onSend: vi.fn().mockResolvedValue(undefined) });
    await click(host.querySelector('[aria-label="Comment on line 2"]'));
    const send = Array.from(host.querySelectorAll('button')).find((b) => b.textContent === 'Send to Claude Code');
    expect((send as HTMLButtonElement).disabled).toBe(true);
  });

  it('shows no send or comment controls when there is no agent to send to', async () => {
    const onSend = vi.fn();
    await mount({ findings: [FINDING], agentLabel: null, onSend });
    expect(host.querySelectorAll('button')).toHaveLength(0);
    expect(host.querySelector('.ck-find')?.textContent).toContain('Missing null check');
    expect(host.querySelector('.ck-gutter--mark')).not.toBeNull();
    expect(onSend).not.toHaveBeenCalled();
  });

  it('does not render unanchored findings', async () => {
    const elsewhere: ReviewFinding = { ...FINDING, id: 'f3', file: 'other.txt' };
    const outside: ReviewFinding = { ...FINDING, id: 'f4', line: 99 };
    await mount({ findings: [elsewhere, outside] });
    expect(host.querySelector('.ck-find')).toBeNull();
    expect(host.querySelector('.ck-gutter--mark')).toBeNull();
  });
});
