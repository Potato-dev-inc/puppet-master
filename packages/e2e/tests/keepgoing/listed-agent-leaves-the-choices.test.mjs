// An agent that is already listed cannot be added twice: it leaves the "Agent to add" choices, and returns when removed.
import { addOffer, keepGoingReset, offerOrder, openKeepGoing } from './_area.mjs';
import { scrollToCss } from '../night/_area.mjs';

export const meta = {
  id: 'keepgoing.listed-agent-leaves-the-choices',
  title: 'A listed agent cannot be added again and comes back to the choices when removed',
  area: 'keepgoing',
  tags: ['keepgoing'],
  edge: true,
};

const choices = (t) => t.page.eval(`[...document.querySelector('select[aria-label="Agent to add"]').options].map((o) => o.text)`);
const addDisabled = (t) => t.page.eval(`[...document.querySelectorAll('button')].find((b) => b.innerText.trim() === 'Add' && b.closest('div')?.querySelector('select[aria-label="Agent to add"]')).disabled`);

export async function run(t) {
  try {
    await openKeepGoing(t);

    await t.step('Add is off until an agent is chosen');
    t.expect.ok((await choices(t)).includes('Codex CLI'), 'Codex CLI can be added');
    t.expect.equal(await addDisabled(t), true);

    await t.step('Add Codex CLI: it is no longer a choice');
    await addOffer(t, 'Codex CLI');
    await t.expect.eventually(async () => (await offerOrder(t)).join() === 'Codex CLI', { message: 'Codex CLI is listed' });
    t.expect.ok(!(await choices(t)).includes('Codex CLI'), 'Codex CLI left the choices');
    t.expect.equal(await addDisabled(t), true);

    await t.step('Remove it: it is a choice again');
    await scrollToCss(t, 'button[aria-label="Remove Codex CLI"]');
    await t.click('aria=Remove Codex CLI');
    await t.expect.eventually(async () => (await choices(t)).includes('Codex CLI'), { message: 'Codex CLI is back in the choices' });
  } finally {
    await keepGoingReset(t);
  }
}

export const setup = keepGoingReset;
