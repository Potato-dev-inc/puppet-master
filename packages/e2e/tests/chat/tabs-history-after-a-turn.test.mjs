// After a turn that changed a file, History lists the save point from before the turn, with the change and a Restore button.
import { turnScript, think, say, openChat, sendChat } from './_area.mjs';

export const meta = {
  id: 'chat.tabs-history-after-a-turn',
  title: 'History lists the save point before a turn that wrote a file',
  area: 'chat',
  tags: ['tabs'],
  timeout: 120000,
};

export async function run(t) {
  t.fake.script(turnScript([think(300), '{ do = "write_file", path = "out.txt", content = "hi" }', say('wrote it')]));
  await t.spawnPane('nimbus', { cwd: t.project('tabs-history') });
  await openChat(t);

  await t.step('Ask the agent to write a file');
  await sendChat(t, 'write it');
  await t.expect.text('wrote it');

  await t.step('History shows the save point before the turn');
  // The save point is written a moment after the turn ends; the tab reads the list when it opens, so open it again until it is there.
  await t.expect.eventually(
    async () => {
      await t.click('exact=History');
      if ((await t.page.text()).includes('Before turn 1')) return true;
      await t.click('exact=Chat');
      return false;
    },
    { timeout: 60000, interval: 500, message: 'History lists the save point before turn 1' },
  );
  await t.expect.text('Restoring puts your files back.');
  // The row's buttons show when the pointer is on the row, so only their words are checked.
  await t.expect.text('Restore');
  await t.expect.text('Branch from here');
  await t.expect.noText('No checkpoints yet');
}
