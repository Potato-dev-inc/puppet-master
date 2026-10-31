// Real work in a folder whose path has spaces and non-latin characters: the pane, the files and History all cope.
import { calm, S, projectDir, writeFiles, workTurn, openHistoryPoint, historyFiles, fs, path } from './_area.mjs';

export const meta = {
  id: 'work.files-folder-spaces-unicode',
  title: 'A folder path with spaces, accents and Japanese characters works for a turn that writes a file',
  area: 'work',
  tags: ['work', 'files'],
  edge: true,
  timeout: 120000,
};

export async function run(t) {
  await calm(t);
  const cwd = projectDir(t, 'mon projet ünï 日本 x');
  writeFiles(cwd, { 'lisez-moi.txt': 'bonjour\n' });

  await t.step('The agent writes a file with a unicode name in a sub folder of that folder');
  const { pane } = await workTurn(t, { cwd, steps: [S.write('données/résumé 日本.txt', 'ok\n'), S.write('lisez-moi.txt', 'bonjour\nmonde\n'), S.say('fait')] });
  t.expect.ok(fs.existsSync(path.join(cwd, 'données', 'résumé 日本.txt')), 'the file is on disk');
  t.expect.includes((await pane.info()).cwd, 'ünï 日本');

  await t.step('History lists both files by their real names');
  await openHistoryPoint(t, 'do the work', 'After turn 1');
  const rows = await historyFiles(t);
  t.expect.equal(rows.length, 2, JSON.stringify(rows));
  t.expect.ok(rows.some((r) => r.includes('résumé 日本.txt')), JSON.stringify(rows));
  t.expect.ok(rows.some((r) => r.includes('lisez-moi.txt')), JSON.stringify(rows));
}
