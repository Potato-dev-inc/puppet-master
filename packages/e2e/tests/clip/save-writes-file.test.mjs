// Clip It: Save video writes the same file to the place you pick; nothing is sent anywhere.
import fs from 'node:fs';
import path from 'node:path';
import { finishedChatTurn, makeClip, answerSaveDialog } from './_area.mjs';

export const meta = {
  id: 'clip.save-writes-file',
  title: 'Save video writes an MP4 to the chosen path and says nothing was sent anywhere',
  area: 'clip',
  tags: ['clip'],
  timeout: 120000,
};

export async function run(t) {
  const { cwd } = await finishedChatTurn(t);
  const target = path.join(cwd, 'my-clip.mp4');
  const dialog = await answerSaveDialog(t, target);
  try {
    await t.step('Make the clip');
    await makeClip(t);

    await t.step('Press Save video… and choose my-clip.mp4');
    await t.click('Save video');

    await t.step('The sheet says where it was saved and that nothing was sent anywhere');
    await t.expect.text('Nothing was sent anywhere.', { timeout: 30000 });
    await t.expect.text('my-clip.mp4');
    await t.expect.visible('Save another copy');
    t.expect.equal(dialog.asked.length, 1, 'the save window was asked once');

    await t.step('The file is a real MP4 on disk');
    t.expect.ok(fs.existsSync(target), 'the file exists');
    const bytes = fs.readFileSync(target);
    t.expect.ok(bytes.length > 10000, `the file has ${bytes.length} bytes`);
    t.expect.equal(bytes.subarray(4, 8).toString('latin1'), 'ftyp', 'an MP4 starts with an ftyp box');
  } finally {
    await dialog.stop();
  }
}
