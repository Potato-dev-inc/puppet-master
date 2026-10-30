// Make a companion from the sheet: it wakes up, its page opens and its home folder holds the notes.
import fs from 'node:fs';
import path from 'node:path';
import { noCompanions, openSetup, scrollSheetDown, companionList, guarded } from './_area.mjs';
import { PANE_ROW } from '../_helpers.mjs';

export const meta = {
  id: 'companion.make-first',
  title: 'Making a companion wakes it up, opens its page and gives it a folder of its own',
  area: 'companion',
  tags: ['smoke'],
};

export const run = guarded(async (t) => {
  await noCompanions(t);
  await openSetup(t);

  await t.step('Pick the fake agent and wake the companion up');
  await t.click('exact=Nimbus');
  await scrollSheetDown(t);
  await t.click('exact=Wake up Mochi');

  await t.step("Its page opens with its name and says it is here");
  await t.waitFor('exact=Sleep', { timeout: 20000 });
  await t.expect.text('Here. Ask me anything.');
  await t.expect.visible('exact=Mochi', { within: 'css=.ck-side' });

  await t.step("It is a companion, not a plain pane: the sidebar lists no pane for it");
  await t.expect.count(PANE_ROW, 0);

  await t.step('Its notes are in a folder of its own in the app folder');
  const [c] = await companionList(t);
  t.expect.equal(c.agentType, 'nimbus');
  t.expect.match(c.id, /^mochi(-[0-9]+)?$/, 'its id comes from its name');
  t.expect.equal(c.paneId, 'companion-' + c.id);
  t.expect.ok(c.folder.split(path.sep).join('/').toLowerCase().endsWith('puppetmaster/companions/' + c.id), 'the default home is PuppetMaster/companions/<id>');
  for (const f of ['SOUL.md', 'USER.md', 'MEMORY.md']) t.expect.ok(fs.existsSync(path.join(c.folder, f)), `${f} was made`);
  t.expect.ok(fs.readFileSync(path.join(c.folder, 'SOUL.md'), 'utf8').includes('cheerful'), 'the cheerful voice is in SOUL.md');

  await t.step('The agent was started in that folder and read its first message');
  await t.fake.waitEvent('prompt');
});
