// When the agent is gone the message box is switched off and says why; it cannot send into a dead pane.
import { turnScript, think, say, openChat, sendChat } from './_area.mjs';

export const meta = {
  id: 'chat.composer-disabled-agent-exited',
  title: 'The message box is off and explains it when the agent has exited',
  area: 'chat',
  tags: ['composer', 'errors'],
  edge: true,
};

export async function run(t) {
  t.fake.script(turnScript([think(200), say('bye'), '{ do = "exit", code = 2 }'], { on_prompt: 'quit now' }));
  await t.spawnPane('nimbus');
  await openChat(t);
  await sendChat(t, 'quit now');

  await t.step('The box says the agent is not running');
  await t.expect.text('The agent is not running. Open Terminal.');
  t.expect.ok(await t.page.eval(`document.querySelector('textarea[aria-label="Message to the agent"]').disabled`), 'the box is off');
  t.expect.ok(await t.page.eval(`document.querySelector('.ck-chat-send').disabled`), 'Send is off');
  t.expect.equal(t.fake.events().filter((e) => e.ev === 'prompt').length, 1, 'only the first message was ever sent');
}
