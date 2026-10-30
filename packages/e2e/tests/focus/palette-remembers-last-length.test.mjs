// The palette lists the length you started last first, so the next Focus is one Enter away.
import { focusReset } from '../night/_area.mjs';

export const meta = {
  id: 'focus.palette-remembers-last-length',
  title: 'The palette lists the last Focus length first',
  area: 'focus',
  tags: ['focus'],
  edge: true,
};

const order = (t) => t.page.eval(`[...document.querySelectorAll('[role=dialog][aria-label="Command palette"] *')].filter((e) => e.children.length === 0 && /^Focus \\d+ minutes$/.test(e.innerText?.trim() ?? '')).map((e) => e.innerText.trim())`);

export async function run(t) {
  try {
    await t.step('First time: the usual order');
    await t.press('Ctrl+K');
    await t.waitFor('aria=Search commands');
    await t.type('focus');
    await t.expect.eventually(async () => (await order(t)).length === 3, { message: 'three rows' });
    t.expect.equal(await order(t), ['Focus 25 minutes', 'Focus 45 minutes', 'Focus 90 minutes']);

    await t.step('Start 90 minutes, then end it');
    await t.click('exact=Focus 90 minutes');
    await t.waitFor('css=[data-focus=on]');
    await t.click('exact=End Focus', { within: 'css=[data-focus=on]' });
    await t.waitFor('css=[data-focus=off]');

    await t.step('The palette now lists 90 first');
    await t.press('Ctrl+K');
    await t.waitFor('aria=Search commands');
    await t.type('focus');
    await t.expect.eventually(async () => (await order(t))[0] === 'Focus 90 minutes', { message: '90 minutes is first' });
    t.expect.equal(await order(t), ['Focus 90 minutes', 'Focus 25 minutes', 'Focus 45 minutes']);
    await t.press('Escape');
  } finally {
    await focusReset(t);
  }
}

export const setup = focusReset;
