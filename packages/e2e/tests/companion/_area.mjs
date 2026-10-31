// Helpers shared by the companion, runs, security, phone, team and live tests. Files starting with "_" are never run.
import http from 'node:http';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';

/** Kept for old tests: the runner honours meta.skip itself, so this does nothing. */
export function skipKnownBug(t, meta) {
}

/** Call a Tauri command from the cockpit page, the way the UI does. Returns { ok } or { err }. */
export async function invoke(t, cmd, args = {}) {
  return t.page.eval(
    `window.__TAURI_INTERNALS__.invoke(${JSON.stringify(cmd)}, ${JSON.stringify(args)}).then(v => ({ ok: v }), e => ({ err: String((e && e.message) || e) }))`,
  );
}

export async function companionList(t) {
  const r = await invoke(t, 'companion_list');
  if (r.err) throw new Error('companion_list failed: ' + r.err);
  return r.ok;
}

/** Archive every companion and wait until its pane is gone (no page reload). */
export async function archiveCompanions(t) {
  for (const c of await companionList(t)) await invoke(t, 'companion_archive', { id: c.id });
  await t.expect.eventually(async () => !(await t.panes()).some((p) => String(p.id).startsWith('companion-')), { message: 'expected the companion pane to close' });
}

/** Companions are saved in the app's data folder and outlive the reset between tests: remove every one first. */
export async function noCompanions(t) {
  for (const c of await companionList(t)) await invoke(t, 'companion_archive', { id: c.id });
  await t.expect.eventually(async () => (await companionList(t)).length === 0, { message: 'expected no companions' });
  // the archived companion's pane closes with it
  await t.expect.eventually(async () => !(await t.panes()).some((p) => String(p.id).startsWith('companion-')), { message: 'expected the companion pane to close' });
  await t.page.reload();
  await t.expect.text('Make a companion');
}

/** A draft for `companion_create` (the same shape the setup sheet sends). */
export function draft(over = {}) {
  return {
    name: 'Mochi', avatar: 'mochi', persona: 'cheerful', agentType: 'nimbus', folder: '', autoStart: true,
    memoryReview: true, napAt: { h: 4, m: 0 }, checkins: [], ...over,
  };
}

/** Make a companion without the sheet (set-up for tests about something else). Waits until it is up and idle. */
export async function makeCompanion(t, over = {}) {
  const r = await invoke(t, 'companion_create', { input: draft(over) });
  if (r.err) throw new Error('companion_create failed: ' + r.err);
  const id = r.ok.id;
  // idle AND done with its first message: the line under its name stops changing, so no change event comes later
  await t.expect.eventually(async () => { const c = (await companionList(t)).find((x) => x.id === id); return c?.state === 'idle' && c.stateLine === 'Here. Ask me anything.'; }, { timeout: 40000, message: 'expected the companion to be up, idle and settled' });
  return (await companionList(t)).find((c) => c.id === id);
}

/** The companion page is open and the companion is up. */
export async function openCompanion(t, name = 'Mochi') {
  await t.click(`exact=${name}`, { within: 'css=.ck-side' });
  await t.waitFor('exact=Sleep');
}

/** Open "Make your companion" from the sidebar. */
export async function openSetup(t) {
  await t.click('Make a companion');
  await t.waitFor('Make your companion');
  await settled(t, 'css=.ck-companion-setup');
}

/** Wait until a thing has stopped moving (a sheet that slides in, a list that scrolls), so a click lands where it was measured. */
export async function settled(t, target) {
  let last = '';
  await t.expect.eventually(async () => {
    const r = (await t.page.find(target)).rect;
    const now = r ? [r.x, r.y, r.w, r.h].map(Math.round).join(',') : '';
    const same = now !== '' && now === last;
    last = now;
    return same;
  }, { interval: 120, message: 'expected the sheet to stop moving' });
}

/** The setup sheet is taller than the window: scroll it the way a person does, so the buttons at the bottom can be reached. */
export async function scrollSheetDown(t) {
  await t.page.wheel(1200, [630, 400]);
  const top = () => t.page.eval(`(document.querySelector('.ck-companion-setup') || {}).scrollTop`);
  let last = -1;
  await t.expect.eventually(async () => { const now = await top(); const same = now === last; last = now; return same; }, { interval: 150, message: 'expected the sheet to stop scrolling' });
}

/**
 * Test doubles at the page's IPC door. The page's own `invoke` cannot be replaced (it is sealed), so a few requests are
 * answered on their way out, through the DevTools Fetch domain. One hub per page connection holds every double, because
 * Fetch.enable replaces the patterns of an earlier call. The real folder picker is NEVER opened: without a folder set,
 * it answers "cancelled".
 */
const hubs = new WeakMap();
const OK_HEADERS = [
  { name: 'Content-Type', value: 'application/json' }, { name: 'Tauri-Response', value: 'ok' },
  { name: 'Access-Control-Allow-Origin', value: '*' }, { name: 'Access-Control-Expose-Headers', value: '*' },
];
const jsonBody = (v) => Buffer.from(JSON.stringify(v)).toString('base64');

async function hubOf(t) {
  const c = t.page.c;
  let h = hubs.get(c);
  if (h) return h;
  h = { picker: { folder: null }, problem: { id: null, value: null, moveWorks: false, moved: false } };
  hubs.set(c, h);
  c.on(async (m) => {
    if (m.method !== 'Fetch.requestPaused') return;
    const { requestId, request, responseStatusCode, responseHeaders } = m.params;
    const url = request.url;
    const answered = responseStatusCode !== undefined;
    const pass = () => c.send(answered ? 'Fetch.continueResponse' : 'Fetch.continueRequest', { requestId });
    try {
      if (url.includes('project_pick_folder')) {
        if (request.method === 'OPTIONS') {
          // the page is on another origin than the app's IPC address, so the browser asks first
          return await c.send('Fetch.fulfillRequest', { requestId, responseCode: 204, responseHeaders: [
            { name: 'Access-Control-Allow-Origin', value: '*' }, { name: 'Access-Control-Allow-Methods', value: 'POST, OPTIONS' },
            { name: 'Access-Control-Allow-Headers', value: '*' }, { name: 'Access-Control-Max-Age', value: '600' },
          ] });
        }
        const f = h.picker.folder;
        const answer = f === null ? null : { path: f, name: path.basename(f), state: 'folder', last_used_ms: 0 };
        return await c.send('Fetch.fulfillRequest', { requestId, responseCode: 200, responseHeaders: OK_HEADERS, body: jsonBody(answer) });
      }
      const p = h.problem;
      if (!p.id || request.method === 'OPTIONS' || !/companion_(list|get|rehome)/.test(url)) return await pass();
      if (/companion_rehome/.test(url)) {
        if (!p.moveWorks || answered) return await pass();
        p.moved = true;
        return await c.send('Fetch.fulfillRequest', { requestId, responseCode: 200, responseHeaders: OK_HEADERS, body: jsonBody({ id: p.id }) });
      }
      if (!answered) return await pass();
      const raw = await c.send('Fetch.getResponseBody', { requestId });
      let json = JSON.parse(raw.base64Encoded ? Buffer.from(raw.body, 'base64').toString('utf8') : raw.body);
      const patch = (v) => (v && v.id === p.id && !p.moved ? { ...v, homeProblem: p.value } : v);
      json = Array.isArray(json) ? json.map(patch) : patch(json);
      await c.send('Fetch.fulfillRequest', { requestId, responseCode: responseStatusCode, responseHeaders: (responseHeaders || []).filter((x) => !/^content-length$/i.test(x.name)), body: jsonBody(json) });
    } catch (e) {
      if (process.env.E2E_DEBUG) console.error('ipc double:', e.message);
      try { await pass(); } catch { /* the page went away */ }
    }
  });
  await c.send('Fetch.enable', { patterns: [
    { urlPattern: '*project_pick_folder*', requestStage: 'Request' },
    { urlPattern: '*companion_list*', requestStage: 'Response' },
    { urlPattern: '*companion_get*', requestStage: 'Response' },
    { urlPattern: '*companion_rehome*', requestStage: 'Request' },
  ] });
  return h;
}

/** Make `project_pick_folder` (the system folder picker, which a test cannot click) answer with `folder`; null = the person closed it. */
export async function stubFolderPicker(t, folder) {
  (await hubOf(t)).picker.folder = folder;
}

/**
 * Show a companion whose home is "too broad" without having one: the app only finds that out when it loads a saved companion,
 * and a test cannot start the app on a prepared data folder. So the answers of `companion_list` and `companion_get` get a
 * `homeProblem` on their way to the page (page and backend are real). With `moveWorks`, "Move to its own folder" is answered
 * with success and the problem is gone from then on; otherwise the real backend answers (it says the companion already
 * has a folder of its own).
 */
export async function injectHomeProblem(t, id, problem, { moveWorks = false } = {}) {
  Object.assign((await hubOf(t)).problem, { id, value: problem, moveWorks, moved: false });
}

/** Stop adding the home problem (later tests of the run must see the plain answers). */
export function clearHomeProblem(t) {
  const h = hubs.get(t.page.c);
  if (h) h.problem.id = null;
}

/** The bridge with the headers of your choice (Node's fetch refuses some of them). `headers` replace the defaults when `bare` is set. */
export function rawBridge(t, { method = 'GET', path: p = '/health', headers = {}, body, bare = false } = {}) {
  const data = body === undefined ? null : Buffer.from(typeof body === 'string' ? body : JSON.stringify(body));
  const base = bare ? {} : { 'x-pm-local-secret': t.app.secret };
  return new Promise((resolve, reject) => {
    const req = http.request({ host: '127.0.0.1', port: t.app.bridgePort, method, path: p, headers: { ...base, ...(data ? { 'content-type': 'application/json', 'content-length': data.length } : {}), ...headers }, timeout: 20000 }, (res) => {
      const chunks = [];
      res.on('data', (c) => chunks.push(c));
      res.on('end', () => {
        const text = Buffer.concat(chunks).toString('utf8');
        let json = null;
        try { json = text ? JSON.parse(text) : null; } catch { /* plain text */ }
        resolve({ status: res.statusCode, ok: res.statusCode < 300, body: json ?? text, text, headers: res.headers });
      });
    });
    req.on('error', reject);
    req.on('timeout', () => req.destroy(new Error('the bridge did not answer')));
    if (data) req.write(data);
    req.end();
  });
}

/** An MCP connection to the bridge: every call carries this session id, as the MCP shim does. */
export function mcp(t, session = '7e57c0de-0000-4000-8000-0000000000a1') {
  const call = (method, p, body, headers = {}) => rawBridge(t, { method, path: p, body, headers: { 'x-puppet-master-session': session, ...headers } });
  return {
    session,
    call,
    run: (body) => call('POST', '/agents/run', { wait_ms: 1, background: true, keep_pane: true, ...body }),
    wait: (project, handles, until = ['completed', 'failed', 'cancelled', 'needs_input'], waitMs = 25000) =>
      call('POST', '/agents/wait', { project_path: project, handles: [].concat(handles), until, wait_ms: waitMs }),
    followup: (project, handle, task, extra = {}) => call('POST', '/agents/followup', { project_path: project, handle, task, wait_ms: 1, background: true, ...extra }),
    answer: (body) => call('POST', '/agents/answer', body),
    cancel: (project, handle) => call('POST', '/agents/cancel', { project_path: project, handle }),
    close: (project, handle) => call('POST', '/agents/close', { project_path: project, handle }),
    get: (p) => call('GET', p),
  };
}

/** The first run view of an `/agents/wait` answer. */
export const firstRun = (waitReply) => waitReply.body?.agents?.[0] ?? null;

/** A script for the fake agent that does one task and says `reply`. */
export const taskScript = (reply, thinkMs = 500) => `
boot_ms = 100
inline = true
[[turn]]
steps = [
  { do = "think", ms = ${thinkMs} },
  { do = "say", text = "${reply}" },
]
`;

/** The text of a file, or null. */
export function readText(file) {
  try { return fs.readFileSync(file, 'utf8'); } catch { return null; }
}

/** One fake agent that behaves by the words in the task: plain, "ask first" (a [y/n] question), "crash now", "exit now", "slow". */
export const RUN_SCRIPT = `
boot_ms = 100
inline = true
[[turn]]
on_prompt = "ask first"
steps = [
  { do = "think", ms = 1500 },
  { do = "approve", kind = "bash", command = "echo hi", on_allow = [{ do = "say", text = "the command ran" }], on_deny = [{ do = "say", text = "the command was refused" }] },
]
[[turn]]
on_prompt = "crash now"
steps = [
  { do = "crash", message = "boom", code = 101 },
]
[[turn]]
on_prompt = "exit now"
steps = [
  { do = "say", text = "bye" },
  { do = "exit", code = 3 },
]
[[turn]]
on_prompt = "slow"
steps = [
  { do = "think", ms = 30000 },
  { do = "say", text = "late answer" },
]
[[turn]]
on_prompt = "second job"
steps = [
  { do = "say", text = "second job done" },
]
[[turn]]
steps = [
  { do = "think", ms = 400 },
  { do = "say", text = "all done: first job" },
]
`;

/**
 * Close every pane that is not a companion's. The harness's own reset uses DELETE /panes/<id>, which the bridge refuses for a pane
 * an MCP run made (403 AUTHORIZATION_DENIED: only the run's owner or the person may close it), so those would pile up. The
 * person's own "Close" is the app command, which is what this calls.
 */
export async function cleanPanes(t) {
  for (const p of await t.panes()) {
    if (String(p.id).startsWith('companion-')) continue;
    await invoke(t, 'kill_pane_cmd', { paneId: p.id });
  }
  await t.expect.eventually(async () => !(await t.panes()).some((p) => !String(p.id).startsWith('companion-')), { message: 'expected the panes to close' });
}

/** Wrap a test body: clean before and after, so run panes never reach the next test. */
export function guarded(body) {
  return async (t) => {
    await cleanPanes(t);
    // The app shows a "While you were away" page after 5 minutes without a real keyboard or mouse event, which a long run never sends.
    const before = (await invoke(t, 'settings_patch', { patch: {} })).ok?.away_minutes;
    await invoke(t, 'settings_patch', { patch: { away_minutes: 0 } });
    if (await t.page.has('exact=While you were away')) await t.click('exact=Got it');
    try { return await body(t); } finally {
      await invoke(t, 'settings_patch', { patch: { away_minutes: before ?? null } }).catch(() => {});
      await cleanPanes(t).catch(() => {});
      await archiveCompanions(t).catch(() => {}); // the next test (maybe another area) starts without a companion
      await forgetPhones(t).catch(() => {});
      await keepWelcomeOff(t).catch(() => {}); // a Settings page may have been opened
    }
  };
}

/** Wait until a run is blocked on a question the caller can answer: needs_input AND the question has its id. */
export async function waitBlocked(t, m, project, handle) {
  let last = null;
  await t.expect.eventually(async () => {
    last = firstRun(await m.wait(project, handle, undefined, 2000));
    if (last?.status === 'needs_input' && !last.prompt?.id) { t.log('needs_input before the question had an id:', JSON.stringify(last.prompt)); const b = await t.bridge.get('/panes/' + last.prompt.pane_id + '/buffer'); t.log('the pane showed:', JSON.stringify(b.body).slice(0, 700)); }
    if (last?.status === 'failed' && last.error?.code === 'OPERATION_NOT_DISPATCHING') return true; // the start race, see askingRun
    return last?.status === 'needs_input' && last.prompt?.id;
  }, { timeout: 30000, message: () => `expected the run to wait for an answer (last view: ${JSON.stringify(last)?.slice(0, 300)})` });
  return last;
}

/**
 * Scroll an element into view with its own scrollIntoView. The page driver can only scroll to things it can already see at
 * their centre, so a field far down a long page (Settings) is never reached by a plain click.
 */
export async function reveal(t, selector, nth = 0) {
  const js = `(() => { const el = document.querySelectorAll(${JSON.stringify(selector)})[${nth}]; if (!el) return null; el.scrollIntoView({ block: 'center' }); const b = el.getBoundingClientRect(); return { y: b.y, h: b.height, vh: innerHeight }; })()`;
  await t.expect.eventually(async () => {
    const r = await t.page.eval(js);
    return r && r.y >= 0 && r.y + r.h <= r.vh;
  }, { interval: 250, message: `could not scroll ${selector} into view` });
  await settled(t, `css=${selector}`);
}

/**
 * Opening a Settings page makes the app forget, in memory, that the first-run welcome was finished (the settings file still
 * says so), and the next page load would cover the cockpit with the welcome. Say it again after visiting Settings.
 */
export const keepWelcomeOff = (t) => invoke(t, 'settings_patch', { patch: { onboarding_done: true } });

/** Open a Settings page by its name in the settings menu (General, Phone, Bridge & MCP ...). */
export async function openSettingsPage(t, page, waitForText) {
  await t.click('exact=Settings', { within: 'css=.ck-side' });
  // the menu can still be drawing when the first click lands: click the page name until its content shows
  await t.expect.eventually(async () => {
    if (await t.page.has(waitForText)) return true;
    await t.click(`exact=${page}`);
    return t.page.has(waitForText);
  }, { timeout: 20000, message: `expected Settings > ${page} to open` });
  await keepWelcomeOff(t);
}

/** Open Settings > General (a long page). */
export const openGeneralSettings = (t) => openSettingsPage(t, 'General', 'Early features');

/** Open the Go live sheet for the pane in the sidebar row `nth`, through its Chat menu (the way a person does). */
export async function openGoLive(t, nth = 0) {
  await t.click('css=.ck-side .ck-row:not(.ck-companion-row):not(.ck-row--flat)', { nth });
  await t.click('exact=Chat');
  await t.click('aria=Chat menu');
  await t.click('Go live');
  await t.waitFor('exact=Go live', { within: 'css=.ck-sheet--live' });
  await settled(t, 'css=.ck-sheet--live');
}

/** Live Room back to its factory settings (no relay address, 1 hour, 10 viewers). */
export const resetLiveRoom = (t) => invoke(t, 'live_room_config_set', { cfg: { relay: '', minutes: 60, maxViewers: 10 } });

/** A phone's public key as the pairing call wants it: a real Ed25519 key, raw, in base64. */
export function devicePublicKey() {
  const { publicKey } = crypto.generateKeyPairSync('ed25519');
  return Buffer.from(publicKey.export({ format: 'jwk' }).x, 'base64url').toString('base64');
}

/**
 * A phone pairs the way the real one does: the person makes a one-time code on the computer, the phone sends it to
 * POST /pair with its own public key, from outside (so the call carries a forwarder header). Returns the device and its token.
 */
export async function pairPhone(t, { name = 'Test phone', caps = ['watch', 'answer', 'type', 'control'] } = {}) {
  const made = await invoke(t, 'create_mobile_pairing_session', { bridgeUrl: `http://127.0.0.1:${t.app.bridgePort}`, caps });
  if (made.err) throw new Error('could not make a pairing code: ' + made.err);
  const code = made.ok.pairing_code;
  const key = devicePublicKey();
  const r = await rawBridge(t, { method: 'POST', path: '/pair', headers: { 'X-Forwarded-For': '203.0.113.50' }, body: { pairing_code: code, device_name: name, device_public_key: key } });
  return { code, reply: r, id: r.body?.device_id, token: r.body?.device_token };
}

/** Call the bridge as a paired phone (a forwarded request with its bearer token). */
export const asPhone = (t, phone, { method = 'GET', path: p, body, headers = {} } = {}) =>
  rawBridge(t, { method, path: p, body, bare: true, headers: { 'X-Forwarded-For': '203.0.113.50', Authorization: `Bearer ${phone.token}`, ...headers } });

/** Sign every paired phone out (they would otherwise stay in the app's data for the next test). */
export async function forgetPhones(t) {
  const list = (await invoke(t, 'list_paired_mobile_devices')).ok || [];
  for (const d of list) await invoke(t, 'revoke_paired_mobile_device', { deviceId: d.id });
}

/**
 * Start a run whose agent asks a question and wait until the question can be answered. Under load the app can call a pane that
 * is still drawing its first screen "needs_input / manual_input_required" and then fail the run with OPERATION_NOT_DISPATCHING
 * (seen on this branch right after other panes were busy; reported as a finding). The run is started again, at most twice.
 */
export async function askingRun(t, m, name) {
  for (let attempt = 1; ; attempt++) {
    const project = t.project(attempt === 1 ? name : name + '-' + attempt);
    const handle = (await m.run({ agent_type: 'nimbus', project_path: project, task: 'ask first please' })).body.handle;
    const view = await waitBlocked(t, m, project, handle);
    if (view.status === 'needs_input') return { project, handle, blocked: view };
    t.log('attempt ' + attempt + ': the run failed at start with ' + view.error?.code);
    if (attempt >= 3) throw new Error('the run failed at start three times: ' + JSON.stringify(view.error));
    await cleanPanes(t);
  }
}

/** Reload and open the companion page until the injected home problem shows (a change event can replace the list once). */
export async function openCompanionWithProblem(t, text, name = 'Mochi') {
  await t.expect.eventually(async () => {
    await t.page.reload();
    await openCompanion(t, name);
    return t.page.has(text);
  }, { timeout: 30000, interval: 500, message: 'expected the home problem card to show' });
}
