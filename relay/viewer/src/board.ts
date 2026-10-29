// Draws the model into the page. Only createElement, textContent and classes are used, so the page never
// parses text from the relay as markup.

import {
  durationLabel,
  noticeFor,
  resultLabel,
  stateWord,
  toolMeta,
  type Model,
} from './render';
import type { ItemView, PaneView } from './types';

export interface BoardElements {
  title: HTMLElement;
  live: HTMLElement;
  notice: HTMLElement;
  tabs: HTMLElement;
  panes: HTMLElement;
  follow: HTMLButtonElement;
}

export interface BoardOptions {
  onSelect: (key: string) => void;
  onFollow: (on: boolean) => void;
}

/** What the viewer chose on the page: which pane shows on narrow screens, and whether to follow new rows. */
export interface BoardUi {
  active: string | null;
  follow: boolean;
}

function make<K extends keyof HTMLElementTagNameMap>(
  doc: Document,
  tag: K,
  className?: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const node = doc.createElement(tag);
  if (className) {
    node.className = className;
  }
  if (text !== undefined) {
    node.textContent = text;
  }
  return node;
}

function itemRow(doc: Document, item: ItemView): HTMLElement {
  switch (item.t) {
    case 'user': {
      const row = make(doc, 'li', 'lr-row lr-row--user');
      row.appendChild(make(doc, 'div', 'lr-bubble', item.text));
      return row;
    }
    case 'agent': {
      const row = make(doc, 'li', 'lr-row lr-row--agent');
      row.appendChild(make(doc, 'div', 'lr-text', item.text));
      return row;
    }
    case 'tool': {
      const row = make(doc, 'li', 'lr-tool');
      row.appendChild(make(doc, 'span', 'lr-tool-name', item.name));
      if (item.summary) {
        row.appendChild(make(doc, 'span', 'lr-tool-summary', item.summary));
      }
      const meta = toolMeta(item);
      if (meta) {
        row.appendChild(make(doc, 'span', 'lr-tool-meta', meta));
      }
      return row;
    }
    case 'result':
      return make(doc, 'li', `lr-result${item.ok ? '' : ' lr-result--failed'}`, resultLabel(item));
    case 'end': {
      const parts = [item.outcome, durationLabel(item.ms)].filter((part) => part !== '');
      return make(doc, 'li', 'lr-end', parts.join(' · '));
    }
    case 'plan': {
      const row = make(doc, 'li', 'lr-plan');
      const list = make(doc, 'ul', 'lr-plan-list');
      for (const task of item.tasks) {
        const line = make(doc, 'li', 'lr-task', task.text);
        if (task.status) {
          line.setAttribute('data-status', task.status);
        }
        list.appendChild(line);
      }
      row.appendChild(list);
      return row;
    }
    case 'note':
      return make(doc, 'li', 'lr-note', item.text);
  }
}

function paneColumn(doc: Document, pane: PaneView, items: ItemView[], active: boolean): HTMLElement {
  const column = make(doc, 'section', active ? 'lr-pane lr-pane--active' : 'lr-pane');
  column.setAttribute('data-key', pane.key);
  column.setAttribute('data-state', pane.state);

  const head = make(doc, 'header', 'lr-pane-head');
  head.appendChild(make(doc, 'h2', 'lr-pane-label', pane.label || 'Agent'));
  if (pane.title) {
    head.appendChild(make(doc, 'p', 'lr-pane-title', pane.title));
  }
  const state = make(doc, 'p', 'lr-state');
  if (pane.state === 'waiting') {
    state.appendChild(make(doc, 'span', 'lr-pill', pane.line || 'Waiting for the owner'));
  } else {
    state.appendChild(make(doc, 'span', 'lr-state-word', stateWord(pane.state)));
    if (pane.line) {
      state.appendChild(make(doc, 'span', 'lr-line', pane.line));
    }
  }
  head.appendChild(state);
  column.appendChild(head);

  const chat = make(doc, 'ol', 'lr-chat');
  if (items.length === 0) {
    chat.appendChild(make(doc, 'li', 'lr-empty', 'Nothing shared yet'));
  }
  for (const item of items) {
    chat.appendChild(itemRow(doc, item));
  }
  column.appendChild(chat);
  return column;
}

export class Board {
  private readonly doc: Document;
  private readonly els: BoardElements;
  private readonly onSelect: (key: string) => void;

  constructor(doc: Document, els: BoardElements, options: BoardOptions) {
    this.doc = doc;
    this.els = els;
    this.onSelect = options.onSelect;
    els.follow.addEventListener('click', () => {
      options.onFollow(els.follow.getAttribute('aria-pressed') !== 'true');
    });
  }

  /** Redraws the page. Returns the pane key that is shown on narrow screens. */
  draw(model: Model, ui: BoardUi): string | null {
    const { doc, els } = this;
    const keys = model.panes.map((pane) => pane.key);
    const active = ui.active !== null && keys.includes(ui.active) ? ui.active : (keys[0] ?? null);

    els.title.textContent = model.room?.title || 'Live Room';
    els.live.hidden = model.status !== 'live' && model.status !== 'reconnecting';
    // The pill follows the room: while the owner has paused it, it must not say LIVE.
    const paused = model.room?.paused === true;
    els.live.textContent = paused ? 'PAUSED' : 'LIVE';
    els.live.toggleAttribute('data-paused', paused);
    const notice = noticeFor(model);
    els.notice.textContent = notice;
    els.notice.hidden = notice === '';
    els.notice.setAttribute('data-status', model.status);

    els.tabs.replaceChildren();
    for (const pane of model.panes) {
      const tab = make(doc, 'button', 'lr-tab', pane.label || 'Agent');
      tab.type = 'button';
      tab.setAttribute('data-key', pane.key);
      tab.setAttribute('aria-pressed', String(pane.key === active));
      tab.addEventListener('click', () => this.onSelect(pane.key));
      els.tabs.appendChild(tab);
    }

    els.panes.replaceChildren();
    for (const pane of model.panes) {
      const items = model.items.filter((item) => item.pane === pane.key);
      els.panes.appendChild(paneColumn(doc, pane, items, pane.key === active));
    }

    els.follow.setAttribute('aria-pressed', String(ui.follow));
    if (ui.follow) {
      for (const chat of els.panes.querySelectorAll<HTMLElement>('.lr-chat')) {
        chat.scrollTop = chat.scrollHeight;
      }
    }
    return active;
  }
}
