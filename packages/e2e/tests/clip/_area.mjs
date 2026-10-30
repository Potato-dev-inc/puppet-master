// Helpers for the Clip It tests: an agent whose Chat has finished turns ("ember"), and a stand-in for the system save dialog.
import { installAgent, emberTurn, openChatTab, selectByTitle, projectDir } from '../whatbroke/_area.mjs';

export * from '../whatbroke/_area.mjs';

/** A folder, an installed "ember" agent, one finished turn in its Chat, the Chat tab open. Returns the pane and folder. */
export async function finishedChatTurn(t, { prompt = 'Add a greeting to main.py', answer = 'Added the greeting to main.py.', folder = 'greeter', tool = true } = {}) {
  await installAgent(t, { id: 'ember', label: 'Ember', transcript: true });
  t.fake.script(`agent = "nimbus"\nboot_ms = 100\ninline = true\n[[turn]]\nsteps = [\n  { do = "think", ms = 1200 },\n${tool ? '  { do = "tool", label = "read_file", detail = "main.py", ms = 300 },\n' : ''}  { do = "say", text = ${JSON.stringify(answer)} },\n]\n`);
  const cwd = projectDir(t, folder);
  const { pane } = await emberTurn(t, { cwd, prompt, answer });
  await t.click('css=.ck-side .ck-row:not(.ck-companion-row):not(.ck-row--flat)', { nth: 0 });
  await openChatTab(t);
  await t.expect.text('Make clip');
  return { pane, cwd };
}

const CORS = [
  { name: 'Access-Control-Allow-Origin', value: '*' },
  { name: 'Access-Control-Allow-Headers', value: '*' },
  { name: 'Access-Control-Allow-Methods', value: '*' },
  { name: 'Access-Control-Expose-Headers', value: '*' },
];

/**
 * Stand in for the system "save as" window (a native dialog a test cannot reach). The app asks its shell over a local
 * request; this answers that one request with `answer` (a file path, or null for "the person cancelled"). Everything
 * else goes through untouched. Call `stop()` when the test is done.
 */
export async function answerSaveDialog(t, answer) {
  const conn = t.page.c;
  const asked = [];
  const off = conn.on(async (m) => {
    if (m.method !== 'Fetch.requestPaused') return;
    const { requestId, request } = m.params;
    try {
      if (request.method === 'OPTIONS') {
        await conn.send('Fetch.fulfillRequest', { requestId, responseCode: 200, responseHeaders: CORS, body: '' });
      } else {
        asked.push(request.postData || '');
        await conn.send('Fetch.fulfillRequest', {
          requestId,
          responseCode: 200,
          responseHeaders: [...CORS, { name: 'Content-Type', value: 'application/json' }, { name: 'Tauri-Response', value: 'ok' }],
          body: Buffer.from(JSON.stringify(answer)).toString('base64'),
        });
      }
    } catch { /* the page went away */ }
  });
  await conn.send('Fetch.enable', { patterns: [{ urlPattern: '*dialog*' }] });
  return {
    asked,
    async stop() {
      off();
      await conn.send('Fetch.disable').catch(() => {});
    },
  };
}

/** Open the sheet from the newest turn's "Make clip" button and wait until the video is drawn. */
export async function makeClip(t) {
  await t.click('exact=Make clip');
  await t.waitFor('Make a clip');
  await t.expect.text('Look at it before you share it.', { timeout: 60000 });
}

/** What the clip preview's video element reports. */
export const clipVideo = (t) => t.page.eval(`(()=>{const v=document.querySelector('video[aria-label="Clip preview"]');return v?{duration:v.duration,w:v.videoWidth,h:v.videoHeight,ready:v.readyState}:null})()`);
