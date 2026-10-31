// Real work summarised: the Receipt tab's "Last turn" sentence counts the files and lines the turn changed.
import { calm, S, gitProject, workTurn, selectByTitle } from './_area.mjs';

export const meta = {
  id: 'work.files-receipt-sentence',
  title: 'The Receipt says in one sentence how many files and lines the last turn changed',
  area: 'work',
  tags: ['work', 'files', 'receipt'],
  timeout: 120000,
};

export async function run(t) {
  await calm(t);
  const cwd = gitProject(t, 'receipt', { 'a.txt': 'one\ntwo\n' });

  await t.step('The agent edits a.txt (+2 lines, 1 line replaced) and adds b.txt');
  await workTurn(t, { cwd, steps: [S.write('a.txt', 'one\nTWO\nthree\n'), S.write('b.txt', 'b\n'), S.say('done')] });

  await t.step('Receipt > Last turn: the sentence names the agent, two files, and +3 -1');
  await selectByTitle(t, 'do the work');
  await t.click('exact=Receipt');
  await t.expect.text('Last turn');
  await t.expect.eventually(async () => /Nimbus edited 2 files \(\+3 .1\), now idle\./.test(await t.page.text()), {
    timeout: 30000,
    message: async () => `the sentence; the page says: ${(await t.page.text()).split('\n').filter((l) => /edited|approved|changes/.test(l)).join(' | ')}`,
  });
  await t.expect.text('Turn 1');
}
