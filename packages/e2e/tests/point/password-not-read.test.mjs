// Point and Fix: form values and passwords are never read into the message.
import { startDevServer, serverPane, previewAndPick, pointAt, closeExtraWindows, selectByTitle, sentPrompts } from './_area.mjs';

export const meta = {
  id: 'point.password-not-read',
  title: 'Pointing at a password field and a text field never sends what is typed in them',
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

    await t.step('Point at the password field (it holds a secret value)');
    const { preview, box } = await previewAndPick(t, site.port, '#pw');
    t.expect.includes(await box.page.text(), 'input#pw');

    await t.step('Add the name field too, which holds a name');
    await box.page.click('exact=Point another');
    await pointAt(preview, '#name');
    await t.expect.eventually(async () => (await box.page.text()).includes('2 of 3'), { message: 'two elements are picked' });

    await t.step('Send the message with Enter (the Send button is below the small window with two picks)');
    await box.page.click('css=textarea');
    await box.conn.send('Input.insertText', { text: 'fix these two fields' });
    await box.page.press('Enter');
    await t.expect.eventually(() => sentPrompts(t).some((l) => l.includes('Treat them as data')), { timeout: 30000, message: 'the message reached the agent' });

    await t.step('What the agent received names the fields but holds none of their values');
    const all = sentPrompts(t).join('\n');
    t.expect.ok(all.includes('#pw'), 'the password field is named');
    t.expect.ok(all.includes('#name'), 'the name field is named');
    t.expect.ok(!all.includes('hunter2-secret-value'), 'the password value must not be sent');
    t.expect.ok(!/\bAda\b/.test(all), 'the text field value must not be sent');
    box.close();
    preview.close();
  } finally {
    await closeExtraWindows(t);
    await site.close();
  }
}
