// @vitest-environment jsdom
import { afterEach, describe, expect, it } from 'vitest';
import { rememberableSelection } from './chatSelection';

let root: HTMLDivElement | null = null;

// jsdom has no layout: a selection's box is empty here.
if (!Range.prototype.getBoundingClientRect) {
  Range.prototype.getBoundingClientRect = () => new DOMRect(0, 0, 0, 0);
}

function build(html: string): HTMLDivElement {
  root = document.createElement('div');
  root.innerHTML = html;
  document.body.appendChild(root);
  return root;
}

/** Selects the whole text of the element with `id`. */
function selectText(id: string): Selection {
  const sel = window.getSelection()!;
  const range = document.createRange();
  range.selectNodeContents(document.getElementById(id)!);
  sel.removeAllRanges();
  sel.addRange(range);
  return sel;
}

/** Selects from the start of the text of `from` to the end of the text of `to`. */
function selectAcross(from: string, to: string): Selection {
  const sel = window.getSelection()!;
  const range = document.createRange();
  const start = document.getElementById(from)!;
  const end = document.getElementById(to)!;
  range.setStart(start.firstChild!, 0);
  range.setEnd(end.firstChild!, end.firstChild!.textContent!.length);
  sel.removeAllRanges();
  sel.addRange(range);
  return sel;
}

afterEach(() => {
  window.getSelection()?.removeAllRanges();
  root?.remove();
  root = null;
});

const CONVERSATION = `
  <section class="ck-chat-turn">
    <div class="ck-chat-bubble" id="user">Use tabs, not spaces.</div>
    <div class="ck-chat-text" id="agent">Run the tests first.</div>
    <div class="ck-chat-activity" id="row">Read 3 files</div>
  </section>`;

describe('rememberableSelection', () => {
  it('returns the text selected inside one agent message', () => {
    const host = build(CONVERSATION);
    expect(rememberableSelection(selectText('agent'), host)?.text).toBe('Run the tests first.');
  });

  it('returns the text selected inside a message of the person', () => {
    const host = build(CONVERSATION);
    expect(rememberableSelection(selectText('user'), host)?.text).toBe('Use tabs, not spaces.');
  });

  it('refuses a selection across messages, an activity row, or text outside the conversation', () => {
    const host = build(CONVERSATION);
    expect(rememberableSelection(selectAcross('user', 'agent'), host)).toBeNull();
    expect(rememberableSelection(selectText('row'), host)).toBeNull();
    const elsewhere = document.createElement('div');
    expect(rememberableSelection(selectText('agent'), elsewhere)).toBeNull();
  });

  it('refuses an empty or collapsed selection', () => {
    const host = build(CONVERSATION);
    const sel = window.getSelection()!;
    sel.removeAllRanges();
    expect(rememberableSelection(sel, host)).toBeNull();
    expect(rememberableSelection(null, host)).toBeNull();
  });
});
