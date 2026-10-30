// Collision Alert: two agents work in one folder and change the same file; the Inbox warns and pauses nothing.
import { twoWriters, ALERT_TITLE } from './_area.mjs';

export const meta = {
  id: 'collision.same-folder-alert',
  title: 'Two agents changing one file in one folder raise an Inbox card that pauses nothing',
  area: 'collision',
  tags: ['collision', 'smoke'],
  timeout: 120000,
};

export async function run(t) {
  await t.step('Two agents start working in the same folder and both change shared.txt');
  const { a, b } = await twoWriters(t);

  await t.step('Open the Inbox: the card names both agents and the file');
  await t.click('exact=Inbox', { within: 'css=.ck-side' });
  await t.expect.text(ALERT_TITLE, { timeout: 90000 });
  await t.expect.eventually(async () => /edit (alpha|beta) and edit (alpha|beta) are working in the same folder/.test(await t.page.text()), { message: 'the card names both agents' });
  await t.expect.text('In the same folder.');
  await t.expect.text('They can overwrite each other. Changed while both worked: shared.txt.');

  await t.step('The card says which one to let finish first, and that nothing was paused');
  await t.expect.eventually(async () => /Let edit (alpha|beta) finish first\. Then ask edit (alpha|beta) to read the file again before it changes more\./.test(await t.page.text()), { message: 'the advice sentence' });
  await t.expect.text('Nothing was paused. You decide.');
  await t.expect.visible('exact=Open edit alpha');
  await t.expect.visible('exact=Open edit beta');
  await t.expect.visible('exact=Ignore');

  await t.step('Both agents are still working: nothing was stopped or typed into');
  t.expect.equal((await a.info()).status, 'running');
  t.expect.equal((await b.info()).status, 'running');
}
