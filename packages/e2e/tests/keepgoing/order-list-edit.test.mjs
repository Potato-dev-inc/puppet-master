// Settings > Agents > Keep going: choose which agents are offered when one runs out of usage, and in which order.
import { addOffer, keepGoingReset, offerOrder, openKeepGoing } from './_area.mjs';
import { scrollToCss } from '../night/_area.mjs';

export const meta = {
  id: 'keepgoing.order-list-edit',
  title: 'The agents offered at a usage limit can be added, reordered and removed',
  area: 'keepgoing',
  tags: ['keepgoing', 'smoke'],
};

export async function run(t) {
  try {
    await t.step('Open Settings > Agents > Keep going');
    await openKeepGoing(t);
    await t.expect.text('Puppet Master offers these agents, in this order, when the agent a task runs on hits its usage limit. You pick one each time.');

    await t.step('Nothing is listed yet, and the page says what happens then');
    await t.expect.text('Nothing listed. Puppet Master then offers every other installed agent, in its usual order.');
    t.expect.equal(await offerOrder(t), []);

    await t.step('Add Codex CLI, then OpenCode');
    await addOffer(t, 'Codex CLI');
    await t.expect.eventually(async () => (await offerOrder(t)).join() === 'Codex CLI', { message: 'Codex CLI is listed' });
    await addOffer(t, 'OpenCode');
    await t.expect.eventually(async () => (await offerOrder(t)).join() === 'Codex CLI,OpenCode', { message: 'both are listed in order' });
    await t.expect.noText('Nothing listed.');

    await t.step('Move OpenCode up: the order flips');
    await scrollToCss(t, 'button[aria-label="Move OpenCode up"]');
    await t.click('aria=Move OpenCode up');
    await t.expect.eventually(async () => (await offerOrder(t)).join() === 'OpenCode,Codex CLI', { message: 'OpenCode is first' });

    await t.step('The first one cannot move up and the last cannot move down');
    t.expect.equal(await t.page.eval(`document.querySelector('button[aria-label="Move OpenCode up"]').disabled`), true);
    t.expect.equal(await t.page.eval(`document.querySelector('button[aria-label="Move Codex CLI down"]').disabled`), true);

    await t.step('Remove OpenCode');
    await t.click('aria=Remove OpenCode');
    await t.expect.eventually(async () => (await offerOrder(t)).join() === 'Codex CLI', { message: 'only Codex CLI is left' });

    await t.step('Leave Settings and come back: the list was saved');
    await t.click('exact=Inbox');
    await openKeepGoing(t);
    t.expect.equal(await offerOrder(t), ['Codex CLI']);
  } finally {
    await keepGoingReset(t);
  }
}

export const setup = keepGoingReset;
