// An agent runs out of usage: a card in the Inbox says so, offers another agent and waits for a tap. Nothing moves by itself.
import { baseSetup, waitClean } from '../night/_area.mjs';
import { claudeHitsItsLimit } from './_limit.mjs';

export const meta = {
  id: 'keepgoing.limit-card-offers-continue',
  title: 'A usage limit shows a card that offers another agent and moves nothing by itself',
  area: 'keepgoing',
  tags: ['keepgoing', 'smoke'],
  timeout: 90000,
};

export async function run(t) {
  await waitClean(t); // the helper pane the launcher used to open the window
  const { undo } = await claudeHitsItsLimit(t, { codex: true });
  try {
    await t.step('The Inbox has a card for the limit');
    await t.expect.text('One thing needs you');
    await t.expect.text('Claude Code ran out of usage.');
    await t.expect.text('It can continue at');

    await t.step('It offers the next agent and the other choices');
    await t.expect.visible('exact=Continue on Codex CLI');
    await t.expect.text('Codex CLI gets a short brief and works in the same folder. Claude Code in this pane is stopped first.');
    await t.expect.visible('exact=Dismiss');
    await t.expect.visible('exact=Open terminal');
    await t.expect.visible('re=/^Remind me at /');

    await t.step('Nothing was started: still one pane and the agent got no more input');
    await t.expect.eventually(async () => (await t.panes()).length === 1, { message: 'only the Claude pane' });
    const events = t.fake.events();
    const afterLimit = events.slice(events.findIndex((e) => e.ev === 'limit') + 1);
    t.expect.equal(afterLimit.filter((e) => ['key', 'prompt'].includes(e.ev)).length, 0, 'nothing typed after the limit');
  } finally {
    undo();
  }
}

export { baseSetup as setup };
