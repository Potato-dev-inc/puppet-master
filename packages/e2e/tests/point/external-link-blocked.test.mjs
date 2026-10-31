// Point and Fix: a link that leaves the local app is stopped; the preview stays on the page and says so.
import { startDevServer, serverPane, previewAndPick, closeExtraWindows, selectByTitle } from './_area.mjs';

export const meta = {
  id: 'point.external-link-blocked',
  title: 'A link to another website in the preview is blocked with a notice and an Open in browser button',
  area: 'point',
  tags: ['point', 'privacy'],
  edge: true,
  timeout: 90000,
};

export async function run(t) {
  const site = await startDevServer();
  try {
    await serverPane(t, site.port);
    await selectByTitle(t, 'serve the app');

    await t.step('Open the preview and pick something so the Point box is open');
    const { preview, box } = await previewAndPick(t, site.port, '#title');

    await t.step('Click the External docs link to example.com in the preview');
    const rect = await preview.eval(`(()=>{const b=document.getElementById('ext').getBoundingClientRect();return {x:b.x+b.width/2,y:b.y+b.height/2}})()`);
    await preview.conn.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: rect.x, y: rect.y });
    await preview.conn.send('Input.dispatchMouseEvent', { type: 'mousePressed', x: rect.x, y: rect.y, button: 'left', clickCount: 1 });
    await preview.conn.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: rect.x, y: rect.y, button: 'left', clickCount: 1 });

    await t.step('The Point box says a link was blocked, and offers Open in browser (only on your click)');
    await t.expect.eventually(async () => (await box.page.text()).includes('example.com'), { timeout: 15000, message: 'the box names the blocked host' });
    t.expect.includes(await box.page.text(), 'Blocked');
    t.expect.includes(await box.page.text(), 'Open in browser');

    await t.step('The preview is still on the local page');
    t.expect.equal(await preview.eval('location.hostname'), 'localhost');
    t.expect.equal(await preview.eval('document.title'), 'Demo shop');
    box.close();
    preview.close();
  } finally {
    await closeExtraWindows(t);
    await site.close();
  }
}
