// Real work at volume: one turn creates 40 files. History lists every one of them and the pane returns to idle.
import { calm, S, gitProject, workTurn, openHistoryPoint, historyFiles, fs, path } from './_area.mjs';

export const meta = {
  id: 'work.files-many-files-one-turn',
  title: 'A turn that creates 40 files lists all 40 in History',
  area: 'work',
  tags: ['work', 'files'],
  edge: true,
  timeout: 120000,
};

export async function run(t) {
  await calm(t);
  const cwd = gitProject(t, 'bulk', { 'README.md': 'demo\n' });
  const maker = path.join(t.app.dirs.root, 'make-forty.cjs');
  fs.writeFileSync(maker, "const fs = require('fs'); fs.mkdirSync('out', { recursive: true }); for (let i = 1; i <= 40; i++) fs.writeFileSync('out/file-' + String(i).padStart(2, '0') + '.txt', 'line ' + i + '\\n');");

  await t.step('The agent runs a generator that creates out/file-01.txt ... out/file-40.txt');
  const { pane } = await workTurn(t, { cwd, steps: [S.run(`node "${maker}"`), S.say('generated forty files')] });
  t.expect.equal(fs.readdirSync(path.join(cwd, 'out')).length, 40);
  t.expect.equal((await pane.info()).status, 'idle');

  await t.step('History: the After row says 40 files and expands to 40 added files');
  await openHistoryPoint(t, 'do the work', 'After turn 1');
  const rows = await historyFiles(t);
  t.expect.equal(rows.length, 40, `got ${rows.length} rows`);
  t.expect.ok(rows.every((r) => /^A out\/file-\d\d\.txt \+1 .0$/.test(r)), JSON.stringify(rows.slice(0, 3)));
}
