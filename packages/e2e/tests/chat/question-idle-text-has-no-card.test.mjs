// A pane that is only idle shows a card for a menu with a cursor, never for plain text or a list with no cursor.
import { turnScript, think, hold, openChat, sendChat } from './_area.mjs';

export const meta = {
  id: 'chat.question-idle-text-has-no-card',
  title: 'Plain text or a list with no cursor on an idle pane gets no question card',
  area: 'chat',
  tags: ['questions'],
  edge: true,
};

export async function run(t) {
  t.fake.script(
    turnScript([think(300), hold('What should the project be called?')], { on_prompt: 'text' }) +
      `
[[turn]]
on_prompt = "list"
steps = [ ${think(300)}, ${hold('Choose a mode\n1. Careful\n2. Fast')} ]
`,
  );
  const pane = await t.spawnPane('nimbus');
  await openChat(t);

  await t.step('The agent stops on a plain question');
  await sendChat(t, 'text');
  await pane.waitScreen('What should the project be called?');
  await t.expect.eventually(async () => (await pane.info()).status === 'idle', { message: 'the pane is idle' });
  await t.expect.text('What should the project be called?');
  await t.expect.noText('Agent is asking');

  await t.step('It stays quiet for a few more reads of the screen');
  await staysAbsent(t, 'Agent is asking', 4000);

  await t.step('A numbered list with no cursor is no card either');
  await pane.key('Enter');
  await t.fake.waitEvent('turn_done');
  await sendChat(t, 'list');
  await pane.waitScreen('2. Fast');
  await t.expect.eventually(async () => (await pane.info()).status === 'idle', { message: 'the pane is idle again' });
  await t.expect.text('2. Fast');
  await staysAbsent(t, 'Agent is asking', 4000);
}

/** The text must not show up during the next `ms`: a card that is going to appear needs time (the screen is read every second or two). */
async function staysAbsent(t, text, ms) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    t.expect.ok(!(await t.page.text()).includes(text), `"${text}" showed up`);
    await t.sleep(300);
  }
}
