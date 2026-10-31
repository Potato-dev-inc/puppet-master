// Point and Fix: with two picks the Send button must still be inside the Point box window.
import { startDevServer, serverPane, previewAndPick, pointAt, closeExtraWindows, selectByTitle, boxClick } from './_area.mjs';

export const meta = {
  id: 'point.send-button-with-two-picks',
  title: 'With two picks the Send button of the Point box is still inside the window',
  area: 'point',
  tags: ['point'],
  edge: true,
  timeout: 90000,
};

export async function run(t) {
  const site = await startDevServer();
  try {
    await serverPane(t, site.port);
    await selectByTitle(t, 'serve the app');
    const { preview, box } = await previewAndPick(t, site.port, '#title');

    await t.step('Point at a second element');
    await boxClick(box, 'Point another');
    await pointAt(preview, '#intro');
    await t.expect.eventually(async () => (await box.page.text()).includes('2 of 3'), { message: 'two picks' });

    await t.step('Send is inside the window');
    const r = await box.eval(`(()=>{const b=[...document.querySelectorAll('button')].find(b=>b.textContent.trim()==='Send').getBoundingClientRect();return {bottom:b.bottom,inner:innerHeight}})()`);
    t.expect.ok(r.bottom <= r.inner, `the Send button ends at ${r.bottom} but the window is ${r.inner} high`);
    box.close();
    preview.close();
  } finally {
    await closeExtraWindows(t);
    await site.close();
  }
}
