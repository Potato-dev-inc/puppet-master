// Clip It: a finished turn in Chat has "Make clip"; the sheet draws a short silent video of it and shows it.
import { finishedChatTurn, makeClip, clipVideo } from './_area.mjs';

export const meta = {
  id: 'clip.make-from-chat-turn',
  title: 'Make clip on a finished chat turn draws a short video and shows it with a warning to look first',
  area: 'clip',
  tags: ['clip', 'smoke'],
  timeout: 120000,
};

export async function run(t) {
  await t.step('An agent finishes a turn; the chat shows its end line with Make clip');
  await finishedChatTurn(t);
  await t.expect.visible('exact=Make clip');

  await t.step('Press Make clip: the sheet starts drawing');
  await t.click('exact=Make clip');
  await t.expect.text('Make a clip');
  await t.expect.text('A short video of this turn. It stays on your PC until you share it. Nothing is uploaded.');

  await t.step('The caption is your request, and the video plays in the sheet');
  await t.expect.value('css=.ck-clip-input', 'Add a greeting to main.py');
  await t.expect.text('Look at it before you share it.', { timeout: 60000 });
  await t.expect.text('It shows what the agent printed on screen.');
  await t.expect.visible('exact=Save video…');

  await t.step('The video is 1280 x 720 and no longer than 28.5 seconds');
  await t.expect.eventually(async () => (await clipVideo(t))?.w > 0, { message: 'the video has loaded' });
  const v = await clipVideo(t);
  t.expect.equal([v.w, v.h], [1280, 720]);
  t.expect.ok(v.duration > 3 && v.duration <= 28.6, `the video length ${v.duration} s is between 3 and 28.5`);
  const facts = (await t.page.text()).match(/(\d+(?:\.\d)?) s · (MP4|WEBM) · [\d.]+ MB/);
  t.expect.ok(facts, 'a line gives the length, the format and the size');
}
