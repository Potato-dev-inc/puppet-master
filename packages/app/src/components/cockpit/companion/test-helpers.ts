// Test helpers for the companion screens: setting the value of a controlled select or input the way React hears it.
import { act } from 'react';

export function setValue(el: HTMLSelectElement | HTMLInputElement, value: string): void {
  const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
  Object.getOwnPropertyDescriptor(proto, 'value')!.set!.call(el, value);
  act(() => {
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? 'change' : 'input', { bubbles: true }));
  });
}
