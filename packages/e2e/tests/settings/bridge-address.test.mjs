// Settings > Bridge & MCP shows the address of the app's own bridge; it is the one the tests talk to, and it answers.
import { calm, openSettings } from './_area.mjs';

export const meta = {
  id: 'settings.bridge-address',
  title: 'Bridge & MCP shows the real address of the running bridge',
  area: 'settings',
  tags: ['bridge'],
};

export async function run(t) {
  await calm(t);
  await openSettings(t, 'Bridge & MCP');

  await t.step('The bridge address is shown');
  const address = `http://127.0.0.1:${t.app.bridgePort}`;
  await t.expect.text('Where other tools reach Puppet Master on this computer.');
  await t.expect.text(address);

  await t.step('It answers on that address');
  const r = await fetch(`${address}/health`);
  t.expect.equal(r.status, 200, 'GET /health');
  t.expect.equal((await r.json()).ok, true, 'the bridge says ok');

  await t.step('The by-hand setup shows the MCP config to copy');
  await t.expect.text('Claude Desktop config');
  await t.expect.text('"puppet-master"');
  await t.expect.text('Codex config');
}
