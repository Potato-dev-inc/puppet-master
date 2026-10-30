// A web page in the person's browser can reach http://127.0.0.1:<port>. The bridge tells those calls from the app's own by
// the Host and Origin headers: a foreign name is remote and needs a token; the app's own origins are served, and only
// those get CORS headers back.
import { rawBridge } from '../companion/_area.mjs';

export const meta = {
  id: 'security.foreign-host-and-origin',
  title: 'A foreign Host or Origin (a web page, DNS rebinding) is refused; the app and localhost are served',
  area: 'security',
  tags: ['security'],
  edge: true,
};

export async function run(t) {
  await t.step('A foreign Host name (DNS rebinding) is refused');
  for (const host of ['evil.example.com', 'evil.example.com:' + t.app.bridgePort, '203.0.113.9']) {
    const r = await rawBridge(t, { path: '/panes', headers: { host } });
    t.expect.equal(r.status, 401, `Host ${host}`);
  }

  await t.step('A foreign Origin (a web page) is refused, including null');
  for (const origin of ['https://evil.example', 'http://evil.example:8080', 'null', 'file://']) {
    const r = await rawBridge(t, { path: '/panes', headers: { origin } });
    t.expect.equal(r.status, 401, `Origin ${origin}`);
    t.expect.equal(r.headers['access-control-allow-origin'], undefined, `no CORS answer for ${origin}`);
  }

  await t.step('The local names are served');
  for (const host of ['127.0.0.1:' + t.app.bridgePort, 'localhost:' + t.app.bridgePort, 'app.localhost', '[::1]:' + t.app.bridgePort]) {
    t.expect.equal((await rawBridge(t, { path: '/panes', headers: { host } })).status, 200, `Host ${host}`);
  }

  await t.step("The app's own origins are served and get a CORS answer naming exactly them");
  for (const origin of ['http://tauri.localhost', 'tauri://localhost']) {
    const r = await rawBridge(t, { path: '/panes', headers: { origin } });
    t.expect.equal(r.status, 200, `Origin ${origin}`);
    t.expect.equal(r.headers['access-control-allow-origin'], origin);
  }

  await t.step("The browser's pre-flight question from a foreign page gets no permission");
  const pre = await rawBridge(t, { method: 'OPTIONS', path: '/panes', headers: { origin: 'https://evil.example', 'access-control-request-method': 'POST' } });
  t.expect.equal(pre.headers['access-control-allow-origin'], undefined, 'no Access-Control-Allow-Origin for a foreign page');
}
