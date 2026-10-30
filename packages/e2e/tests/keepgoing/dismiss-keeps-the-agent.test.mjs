// Dismiss on the limit card closes the card and moves nothing: no second agent starts and the pane stays as it was.
import { baseSetup, sclick, waitClean } from '../night/_area.mjs';
import { claudeHitsItsLimit } from './_limit.mjs';

export const meta = {
  id: 'keepgoing.dismiss-keeps-the-agent',
  title: 'Dismissing the limit card starts no other agent',
  area: 'keepgoing',
  tags: ['keepgoing'],
  edge: true,
  timeout: 90000,
};

export async function run(t) {
  await waitClean(t); // the helper pane the launcher used to open the window
  const { undo } = await claudeHitsItsLimit(t, { codex: true });
  try {
    await t.expect.text('Claude Code ran out of usage.');

    await t.step('Press Dismiss');
    await sclick(t, 'exact=Dismiss');
    await t.expect.noText('Claude Code ran out of usage.');
    await t.expect.text('Nothing needs you');

    await t.step('Only the Claude pane is open and nothing was handed over');
    t.expect.equal((await t.panes()).map((p) => p.agent_type), ['claude']);
    await t.expect.noText('Handed to');
  } finally {
    undo();
  }
}

export { baseSetup as setup };
