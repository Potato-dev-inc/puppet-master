// Point and Fix: only local addresses can be previewed. A docs link in the agent's output is not offered.
import { startDevServer, serveScript, serverPane, useFakeClaude, selectByTitle, projectDir } from './_area.mjs';

export const meta = {
  id: 'point.no-chip-for-public-address',
  title: 'A public web address printed by an agent does not get a Preview chip',
  area: 'point',
  tags: ['point'],
  edge: true,
  skip: "NOT VERIFIED IN THE FULL RUN: it failed in the one full video run after a first-run Welcome screen covered the cockpit mid-run (cause not found); it passed in earlier partial runs. Remove this skip and re-run.",
};

export async function run(t) {
  if (meta.skip) t.skip(meta.skip);
  await t.step('The agent prints a public docs link, not a local server');
  useFakeClaude(t);
  t.fake.script(serveScript('Read more at https://example.com/docs/getting-started and http://203.0.113.9:8080/').replace('on_prompt = "serve the app"', 'on_prompt = "show the docs"'));
  const pane = await t.spawnPane('claude', { cwd: projectDir(t, 'docs'), waitFor: /Claude Code/ });
  await pane.write('show the docs');
  await pane.waitScreen('example.com/docs/getting-started');
  await selectByTitle(t, 'show the docs');

  await t.step('The pane has no Preview chip and the palette has no preview action');
  await t.expect.visible('exact=Terminal');
  await t.expect.notVisible('exact=Preview');
  await t.press('Ctrl+K');
  await t.type('preview');
  await t.expect.noText('Point and fix in the preview');
  await t.expect.noText('Open preview:');
  await t.press('Escape');

  await t.step('Another agent then prints a local address: now its pane has the chip');
  const site = await startDevServer();
  try {
    await serverPane(t, site.port, { cwd: projectDir(t, 'docs-2') });
    await selectByTitle(t, 'serve the app');
    await t.expect.visible('exact=Preview');
  } finally {
    await site.close();
  }
}
