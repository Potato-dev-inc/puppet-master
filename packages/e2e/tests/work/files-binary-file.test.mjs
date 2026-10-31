// Real work: the agent produces a binary file (bytes that are not text). History says "binary" instead of line counts.
import { calm, S, gitProject, workTurn, openHistoryPoint, historyFiles, fs, path } from './_area.mjs';

export const meta = {
  id: 'work.files-binary-file',
  title: 'A binary file the agent creates is listed as binary, with no line counts',
  area: 'work',
  tags: ['work', 'files'],
  edge: true,
  timeout: 120000,
};

export async function run(t) {
  await calm(t);
  const cwd = gitProject(t, 'bin', { 'README.md': 'demo\n' });
  const maker = path.join(t.app.dirs.root, 'make-binary.cjs');
  fs.writeFileSync(maker, "require('fs').writeFileSync('logo.bin', Buffer.from([0, 1, 2, 255, 254, 0, 0, 9, 0, 128]));");

  await t.step('The agent runs a program that writes logo.bin and also edits README.md');
  await workTurn(t, { cwd, steps: [S.run(`node "${maker}"`), S.write('README.md', 'demo\nmore\n'), S.say('made it')] });
  t.expect.equal(fs.statSync(path.join(cwd, 'logo.bin')).size, 10);

  await t.step('History: README.md has line counts, logo.bin says binary');
  await openHistoryPoint(t, 'do the work', 'After turn 1');
  const rows = await historyFiles(t);
  t.expect.equal(rows.length, 2, JSON.stringify(rows));
  t.expect.ok(rows.some((r) => /^M README\.md \+1 .0$/.test(r)), JSON.stringify(rows));
  t.expect.ok(rows.some((r) => /^A logo\.bin binary$/.test(r)), JSON.stringify(rows));
}
