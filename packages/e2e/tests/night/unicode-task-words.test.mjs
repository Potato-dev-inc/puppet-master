// A task's words are shown as text everywhere: unicode and markup-looking characters stay literal, nothing is run as HTML.
import { openMorning, startOneTaskNight, writerScript, nightSetup } from './_area.mjs';

export const meta = {
  id: 'night.unicode-task-words',
  title: 'Task words with unicode and angle brackets stay plain text in the Morning page',
  area: 'night',
  tags: ['night'],
  edge: true,
  timeout: 90000,
};

const WORDS = 'Ünïcode 漢字 émoji 🚀 <b>bold</b> & "quotes" <img src=x onerror=alert(1)>';

export async function run(t) {
  await startOneTaskNight(t, { text: WORDS, script: writerScript('uni.txt', 'ok') });
  await t.waitFor('exact=Open Morning', { timeout: 60000 });

  await t.step('Open the Morning page');
  await openMorning(t);

  await t.step('The words are in the row exactly as typed');
  await t.expect.eventually(async () => (await t.page.eval(`document.querySelector('.ck-morning-title')?.innerText ?? ''`)) === WORDS, { message: 'the row title is the typed text' });

  await t.step('No markup was created from them');
  t.expect.equal(await t.page.eval(`document.querySelectorAll('.ck-morning b, .ck-morning img').length`), 0);
  t.expect.equal(await t.page.eval(`document.querySelector('.ck-morning-title').children.length`), 0);
}

export const setup = nightSetup;
