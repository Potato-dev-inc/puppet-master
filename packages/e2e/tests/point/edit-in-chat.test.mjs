// Point and Fix: "Edit in Chat" puts the message in that agent's chat box as a draft. Nothing is sent.
import { startDevServer, serverPane, previewAndPick, closeExtraWindows, selectByTitle, sentPrompts } from './_area.mjs';

export const meta = {
  id: 'point.edit-in-chat',
  title: 'Edit in Chat puts the pointed element and your words in the agent chat box without sending',
  area: 'point',
  tags: ['point'],
  timeout: 90000,
};

export async function run(t) {
  const site = await startDevServer();
  try {
    await serverPane(t, site.port);
    await selectByTitle(t, 'serve the app');
    const before = sentPrompts(t).length;

    await t.step('Point at the heading and type what should change');
    const { preview, box } = await previewAndPick(t, site.port, '#title');
    await box.page.fill('css=textarea', 'make the heading bigger');

    await t.step('Press Edit in Chat');
    await box.page.click('exact=Edit in Chat');

    await t.step('The cockpit opens that agent chat with the message as a draft in the box');
    await t.waitFor('aria=Message to the agent');
    await t.expect.eventually(async () => {
      const v = await t.page.value('aria=Message to the agent');
      return v.includes('make the heading bigger') && v.includes('Selector: #title');
    }, { message: 'the draft holds your words and the selector' });

    await t.step('Nothing was sent to the agent yet');
    t.expect.equal(sentPrompts(t).length, before);
    box.close();
    preview.close();
  } finally {
    await closeExtraWindows(t);
    await site.close();
  }
}
