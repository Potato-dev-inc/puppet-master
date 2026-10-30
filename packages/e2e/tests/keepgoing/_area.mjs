// Helpers for the Keep going tests. Not a test.
import { baseSetup, invokeCommand, openSettings, scrollToCss } from '../night/_area.mjs';

/** No agent listed (every other agent is offered in the usual order) and "ask to move it back" on: the defaults. */
export async function keepGoingReset(t) {
  await baseSetup(t);
  await invokeCommand(t, 'keep_going_settings_set', { settings: { order: [], ask_back: true } });
}

/** Open Settings > Agents at the Keep going block. */
export async function openKeepGoing(t) {
  await openSettings(t, 'Agents');
  await t.expect.text('When an agent runs out of usage');
  await scrollToCss(t, 'select[aria-label="Agent to add"]');
}

/** The names in the "Offer to continue on" list, in order. */
export const offerOrder = (t) =>
  t.page.eval(`[...document.querySelectorAll('ol[aria-label="Offer order"] li')].map((li) => li.querySelector('.font-medium').innerText)`);

/** Choose an agent in the "Agent to add" list by its name and press Add. */
export async function addOffer(t, name) {
  await t.page.eval(`(() => {
    const sel = document.querySelector('select[aria-label="Agent to add"]');
    const opt = [...sel.options].find((o) => o.text === ${JSON.stringify(name)});
    if (!opt) throw new Error('no ${name} to add');
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(sel, opt.value);
    sel.dispatchEvent(new Event('change', { bubbles: true }));
  })()`);
  await t.click('exact=Add');
}
