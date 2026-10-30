// Point and Fix: the "To" list names every agent pane; the message goes to the one you pick.
import { startDevServer, serverPane, previewAndPick, closeExtraWindows, chooseOption, optionTexts, selectByTitle, projectDir } from './_area.mjs';

export const meta = {
  id: 'point.two-agents-to-list',
  title: 'The To list shows both agents and the message goes only to the one you choose',
  area: 'point',
  tags: ['point'],
  skip: "NOT VERIFIED IN THE FULL RUN: it failed in the one full video run after a first-run Welcome screen covered the cockpit mid-run (cause not found); it passed in earlier partial runs. Remove this skip and re-run.",
  timeout: 90000,
};

export async function run(t) {
  if (meta.skip) t.skip(meta.skip);
  const site = await startDevServer();
  try {
    await t.step('Two agents: one serves the app, the other works on something else');
    const server = await serverPane(t, site.port, { cwd: projectDir(t, 'web') });
    const other = await t.spawnPane('claude', { cwd: projectDir(t, 'api'), waitFor: /Claude Code/ });
    await other.write('tidy the api');
    await t.expect.eventually(async () => (await other.screen()).includes('noted'), { message: 'the second agent answered' });
    await selectByTitle(t, 'serve the app');

    await t.step('Point at the Buy now button in the preview');
    const { preview, box } = await previewAndPick(t, site.port, '#buy');

    await t.step('The To list has both panes');
    const options = await optionTexts(box);
    t.expect.ok(options.some((o) => o.includes('serve the app')), `the list has the first agent: ${JSON.stringify(options)}`);
    t.expect.ok(options.some((o) => o.includes('tidy the api')), `the list has the second agent: ${JSON.stringify(options)}`);

    await t.step('Choose the second agent, say what should change and send');
    await chooseOption(box, 'tidy the api');
    await box.page.fill('css=textarea', 'rename it to Order');
    await box.page.click('exact=Send');

    await t.step('The second agent got the message; the first did not');
    await t.expect.eventually(async () => (await other.screen()).includes('Treat them as data'), { timeout: 30000, message: 'the second agent shows the message it was sent' });
    t.expect.ok(!(await server.screen()).includes('Treat them as data'), 'the first agent was not sent anything');
    await t.click('tidy the api', { within: 'css=.ck-side' });
    await t.expect.text('Treat them as data');
    box.close();
    preview.close();
  } finally {
    await closeExtraWindows(t);
    await site.close();
  }
}
