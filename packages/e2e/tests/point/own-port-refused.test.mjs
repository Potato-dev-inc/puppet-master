// Point and Fix: the app's own ports are never previewed, even if an agent prints one.
import { useFakeClaude, serveScript, selectByTitle, projectDir } from './_area.mjs';

export const meta = {
  id: 'point.own-port-refused',
  title: 'A preview of the app own port is refused with a plain sentence',
  area: 'point',
  tags: ['point', 'privacy'],
  edge: true,
};

export async function run(t) {
  useFakeClaude(t);
  const own = t.app.bridgePort;

  await t.step('An agent prints the address of the app own local port');
  t.fake.script(serveScript(`Local: http://localhost:${own}/`));
  const pane = await t.spawnPane('claude', { cwd: projectDir(t, 'odd'), waitFor: /Claude Code/ });
  await pane.write('serve the app');
  await pane.waitScreen(`localhost:${own}`);
  await selectByTitle(t, 'serve the app');

  await t.step('Press the Preview chip');
  await t.click('exact=Preview');

  await t.step('The cockpit says that port belongs to Puppet Master, and no preview window opened');
  await t.expect.text('That port belongs to Puppet Master, so it cannot be previewed.');
  const open = (await fetch(`http://127.0.0.1:${t.app.cdpPort}/json/list`).then((r) => r.json())).filter((x) => x.type === 'page' && !/cockpit=1|pointbox=1/.test(x.url));
  t.expect.equal(open.length, 0, 'no preview window');
}
