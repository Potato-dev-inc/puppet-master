#!/usr/bin/env node
// Copies the canonical skills (integrations/skills) into every plugin folder that ships them.
// `--check` only compares and exits 1 on drift (the Rust test `pack_files_match_canonical` does the same).
import { cpSync, existsSync, readFileSync, readdirSync, rmSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..', 'integrations');
const source = join(root, 'skills');
const targets = [join(root, 'claude-code/plugin/skills'), join(root, 'codex/plugin/skills'), join(root, 'cursor/plugin/skills')];
const rules = [[join(root, 'cursor/rules/puppet-master.mdc'), join(root, 'cursor/plugin/rules/puppet-master.mdc')]];
const check = process.argv.includes('--check');

function files(dir, base = dir) {
  if (!existsSync(dir)) return [];
  return readdirSync(dir).flatMap((n) => {
    const p = join(dir, n);
    return statSync(p).isDirectory() ? files(p, base) : [p.slice(base.length + 1)];
  });
}

let drift = false;
for (const t of targets) {
  const want = files(source).sort();
  const have = files(t).sort();
  const same = want.join() === have.join() && want.every((f) => readFileSync(join(source, f)).equals(readFileSync(join(t, f))));
  if (same) continue;
  drift = true;
  if (check) console.error(`drift: ${t}`);
  else {
    rmSync(t, { recursive: true, force: true });
    cpSync(source, t, { recursive: true });
    console.log(`synced ${t}`);
  }
}
for (const [from, to] of rules) {
  if (existsSync(to) && readFileSync(from).equals(readFileSync(to))) continue;
  drift = true;
  if (check) console.error(`drift: ${to}`);
  else {
    cpSync(from, to, { recursive: true });
    console.log(`synced ${to}`);
  }
}
if (check && drift) process.exit(1);
if (!drift) console.log('skill pack in sync');
