// A companion's chat is never shared: its pane is listed in the sheet, greyed, with the reason, and cannot be ticked.
import { openGoLive, guarded, noCompanions, makeCompanion } from '../companion/_area.mjs';

export const meta = {
  id: 'live.companion-chat-private',
  title: "A companion's chat is greyed out in Go live: companion chats stay private",
  area: 'live',
  tags: ['security'],
  edge: true,
};

export const run = guarded(async (t) => {
  await noCompanions(t);
  await makeCompanion(t);
  await t.spawnPane('nimbus');
  await t.page.reload();

  await t.step('Open Go live from the normal pane');
  await openGoLive(t);

  await t.step("The companion's pane is there, with the reason");
  await t.expect.text('Companion chats stay private');

  await t.step('Its box cannot be ticked, the normal pane is ticked');
  const boxes = await t.page.eval(`[...document.querySelectorAll('.ck-live-pane')].map(li => ({ text: li.innerText, disabled: li.querySelector('input').disabled, checked: li.querySelector('input').checked }))`);
  const mine = boxes.find((b) => /Companion chats stay private/.test(b.text));
  t.expect.ok(mine, 'the companion row exists');
  t.expect.equal(mine.disabled, true);
  t.expect.equal(mine.checked, false);
  const normal = boxes.find((b) => /Nimbus/.test(b.text) && !/Companion/.test(b.text));
  t.expect.equal(normal.checked, true, 'the pane you are on is ticked by default');
  await noCompanions(t).catch(() => {});
});
