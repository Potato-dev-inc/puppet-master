// Focus never pauses or stops an agent: a message sent during Focus is answered as usual.
import { focusReset, sclick } from '../night/_area.mjs';
import { openChat, replyScript, sendChat } from '../_helpers.mjs';

export const meta = {
  id: 'focus.agents-keep-working',
  title: 'An agent keeps working and answering while Focus is on',
  area: 'focus',
  tags: ['focus'],
  edge: true,
};

export async function run(t) {
  try {
    t.fake.script(replyScript('answered during focus'));
    await t.spawnPane('nimbus');

    await t.step('Start Focus');
    await sclick(t, 'exact=45 min');
    await t.waitFor('css=[data-focus=on]');

    await t.step('Send the agent a message from Chat');
    await openChat(t);
    await sendChat(t, 'are you still there');

    await t.step('It answers, and Focus is still on');
    await t.expect.text('answered during focus');
    t.expect.equal((await t.fake.waitEvent('prompt')).text, 'are you still there');
    await t.expect.visible('css=[data-testid=focus-pill]');
  } finally {
    await focusReset(t);
  }
}

export const setup = focusReset;
