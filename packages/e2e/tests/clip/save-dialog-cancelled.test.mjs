// Clip It: closing the save window without choosing a place saves nothing and shows no error.
import fs from 'node:fs';
import { finishedChatTurn, makeClip, answerSaveDialog } from './_area.mjs';

export const meta = {
  id: 'clip.save-dialog-cancelled',
  title: 'Cancelling the save window saves nothing and leaves the clip ready to save again',
  area: 'clip',
  tags: ['clip'],
  edge: true,
  timeout: 120000,
};

export async function run(t) {
  const { cwd } = await finishedChatTurn(t);
  const dialog = await answerSaveDialog(t, null);
  try {
    await makeClip(t);

    await t.step('Press Save video… and cancel the save window');
    await t.click('Save video');
    await t.expect.eventually(() => dialog.asked.length === 1, { message: 'the save window was asked' });

    await t.step('Nothing was saved, no error, and the button is still there to try again');
    await t.expect.noText('Saved to');
    await t.expect.noText("Couldn't");
    await t.expect.visible('Save video');
    t.expect.equal(fs.readdirSync(cwd).filter((n) => n.endsWith('.mp4')), []);
  } finally {
    await dialog.stop();
  }
}
