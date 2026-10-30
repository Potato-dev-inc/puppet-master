// Point and Fix: up to three elements per message; Discard clears them.
import { startDevServer, serverPane, previewAndPick, pointAt, closeExtraWindows, selectByTitle, boxClick } from './_area.mjs';

export const meta = {
  id: 'point.discard-and-limit',
  title: 'The Point box holds at most three picks and Discard clears them',
  area: 'point',
  tags: ['point'],
  edge: true,
  skip: "NOT VERIFIED IN THE FULL RUN: it failed in the one full video run after a first-run Welcome screen covered the cockpit mid-run (cause not found); it passed in earlier partial runs. Remove this skip and re-run.",
  timeout: 90000,
};

export async function run(t) {
  if (meta.skip) t.skip(meta.skip);
  const site = await startDevServer();
  try {
    await serverPane(t, site.port);
    await selectByTitle(t, 'serve the app');

    await t.step('Pick the heading');
    const { preview, box } = await previewAndPick(t, site.port, '#title');
    t.expect.includes(await box.page.text(), '1 of 3');

    for (const [n, sel] of [[2, '#intro'], [3, '#buy']]) {
      await t.step(`Point another: pick number ${n}`);
      await boxClick(box, 'Point another');
      await pointAt(preview, sel);
      await t.expect.eventually(async () => (await box.page.text()).includes(`${n} of 3`), { message: `${n} of 3` });
    }

    await t.step('With three picks, Point another is off');
    const disabled = await box.eval(`(()=>{const b=[...document.querySelectorAll('button')].find(b=>b.textContent.trim()==='Point another');return !!b&&b.disabled})()`);
    t.expect.ok(disabled, 'Point another is disabled at 3 of 3');

    await t.step('Press Discard: the picks are gone');
    await boxClick(box, 'Discard');
    await t.expect.eventually(async () => (await box.page.text().catch(() => '')).includes('0 of 3'), { message: 'the box is empty again' });
    box.close();
    preview.close();
  } finally {
    await closeExtraWindows(t);
    await site.close();
  }
}
