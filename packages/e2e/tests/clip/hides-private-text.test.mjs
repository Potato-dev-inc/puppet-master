// Clip It: secrets, e-mail addresses and folder paths are hidden before anything is drawn, and the sheet says how many lines.
import { finishedChatTurn, makeClip, tauri } from './_area.mjs';

export const meta = {
  id: 'clip.hides-private-text',
  title: 'A key, an e-mail address and a home folder path in the turn are hidden from the clip',
  area: 'clip',
  tags: ['clip', 'privacy'],
  edge: true,
  timeout: 120000,
};

const KEY = 'sk-test-ABCDEFGHIJKLMNOPQRSTUVWX1234';
const MAIL = 'ana@example.invalid';
const HOME_PATH = 'C:\\Users\\someone\\proj\\secret-notes.rs';

export async function run(t) {
  await t.step('A turn whose request and answer carry a key, an e-mail address and a full path');
  const { pane } = await finishedChatTurn(t, {
    prompt: `Rotate the key ${KEY} and mail ${MAIL}`,
    answer: `Rotated. Wrote ${HOME_PATH} and sent a note to ${MAIL}`,
    folder: 'vault',
    tool: false,
  });

  await t.step('Ask the app for the clip plan: it is what the video is drawn from');
  const plan = await tauri(t, 'clip_plan', { paneId: pane.id, userItemId: null });
  const json = JSON.stringify(plan);
  t.expect.ok(json.length > 500, 'the plan has content');

  await t.step('The plan holds none of the private text, only the last name of the file');
  for (const secret of [KEY, MAIL, 'C:\\\\Users\\\\someone', 'someone\\\\proj', 'ABCDEFGHIJKLMNOP']) t.expect.ok(!json.includes(secret), `the plan must not hold "${secret}"`);
  t.expect.ok(!plan.request.includes('sk-test'), 'the caption is cleaned too');
  t.expect.ok(plan.hidden >= 1, `the plan counts the lines it changed (${plan.hidden})`);

  await t.step('Make the clip: the sheet says lines were hidden and the caption is clean');
  await makeClip(t);
  await t.expect.text('looked private.');
  const caption = await t.page.value('css=.ck-clip-input');
  t.expect.ok(!caption.includes('ABCDEFGHIJKLMNOP') && !caption.includes(MAIL), `the caption "${caption}" has no private text`);
}
