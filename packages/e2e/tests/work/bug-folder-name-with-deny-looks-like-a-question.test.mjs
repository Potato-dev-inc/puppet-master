// BUG (found by a folder name): the app guesses "waiting for you" from words on the agent's screen. A folder called
// "deny-list" (or "approve-queue") is printed in the agent's banner, so an agent that is just idle is reported as asking.
import { calm, projectDir, fs } from './_area.mjs';
import { S, oneTurn } from './_area.mjs';

export const meta = {
  id: 'work.bug-folder-name-with-deny-looks-like-a-question',
  title: 'An idle agent in a folder called "deny-list" is idle, not "waiting for your answer"',
  area: 'work',
  tags: ['work', 'status'],
  edge: true,
  timeout: 90000,
};

export async function run(t) {
  await calm(t);
  for (const name of ['deny-list', 'approve-queue']) {
    const cwd = t.project(name);
    await t.step(`Start an agent in a folder called ${name}: it is just idle`);
    t.fake.script(oneTurn([S.think(300), S.say('hi')], 'approvals = "menus"'));
    const pane = await t.spawnPane('nimbus', { cwd, idle: false });
    await t.sleep(3000);
    t.expect.equal((await pane.info()).status, 'idle', `an idle agent in "${name}" must be idle`);
    await t.click('exact=Inbox', { within: 'css=.ck-side' });
    await t.expect.text('Nothing needs you');
    await pane.kill();
    await t.expect.eventually(async () => (await t.panes()).length === 0, { message: 'pane closed' });
  }
}
