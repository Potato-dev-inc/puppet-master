// Builders and small helpers shared by the Potato Bot tests.
import { act } from 'react';
import type { PotatoBotView, PotatoMessage } from '../../../lib/potato';

export const bot = (over: Partial<PotatoBotView> = {}): PotatoBotView => ({
  id: 'fish',
  name: 'Fish',
  talkPersona: 'fish',
  brainState: 'ready',
  lastError: null,
  typing: false,
  mood: null,
  provider: 'stub',
  unread: 0,
  lastLine: null,
  lastAt: null,
  usedTodayNano: 0,
  limitCents: 50,
  openCards: 0,
  ...over,
});

let counter = 0;
export const msg = (over: Partial<PotatoMessage> & Pick<PotatoMessage, 'text'>): PotatoMessage => ({
  id: `m-${++counter}`,
  at: 1_700_000_000_000 + counter * 1000,
  from: { kind: 'bot', name: 'Fish' },
  kind: 'text',
  ...over,
});

export const you = (text: string): PotatoMessage => msg({ text, from: { kind: 'you' } });
export const note = (text: string): PotatoMessage => msg({ text, from: { kind: 'note' }, kind: 'note' });

export const buttonByText = (host: HTMLElement, label: string): HTMLButtonElement | undefined =>
  [...host.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.trim() === label);

/** Sets a textarea's value the way React hears it. */
export function typeInto(el: HTMLTextAreaElement, value: string): void {
  Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(el, value);
  act(() => {
    el.dispatchEvent(new Event('input', { bubbles: true }));
  });
}
