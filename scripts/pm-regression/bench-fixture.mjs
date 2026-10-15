// Benchmark fixture: a generated tiny CommonJS project, its seeding helpers, and the local
// (ground-truth) evaluation of the machine checks. Every run gets a fresh copy of the fixture.
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';

export const node = process.execPath;
const sha1 = (text) => createHash('sha1').update(text).digest('hex');

// Generated fixture: a tiny CommonJS project. Every run gets a fresh copy so runs never see each
// other's edits. Tasks may seed extra state (a bug, duplicated code) via `setup`.
export const FIXTURE = {
  'package.json': '{ "name": "pm-bench-fixture", "version": "1.0.0", "private": true }\n',
  'src/math.js': `function add(a, b) { return a + b; }
function multiply(a, b) { return a * b; }
function clamp(n, lo, hi) { return Math.min(hi, Math.max(lo, n)); }
function isEven(n) { return n % 2 === 0; }
module.exports = { add, multiply, clamp, isEven };
`,
  'src/format.js': `function formatPrice(cents) { return \`$\${(cents / 100).toFixed(2)}\`; }
function titleCase(text) {
  return text.split(' ').map((word) => word.charAt(0).toUpperCase() + word.slice(1)).join(' ');
}
module.exports = { formatPrice, titleCase };
`,
  'src/cart.js': `const { add, multiply } = require('./math');

function createCart() { return { items: [] }; }
function addItem(cart, item) {
  cart.items.push({ name: item.name, price: item.price, qty: item.qty ?? 1 });
  return cart;
}
function total(cart) {
  return cart.items.reduce((sum, item) => add(sum, multiply(item.price, item.qty)), 0);
}
module.exports = { createCart, addItem, total };
`,
  'src/report.js': `const { total } = require('./cart');
const { formatPrice, titleCase } = require('./format');
const { multiply } = require('./math');

function summarize(cart) {
  const lines = cart.items.map((item) => \`\${titleCase(item.name)} x\${item.qty} \${formatPrice(multiply(item.price, item.qty))}\`);
  lines.push(\`Total: \${formatPrice(total(cart))}\`);
  return lines.join('\\n');
}
module.exports = { summarize };
`,
  'test/math.test.js': `const test = require('node:test');
const assert = require('node:assert');
const { add, multiply, clamp } = require('../src/math');

test('add', () => assert.strictEqual(add(2, 3), 5));
test('multiply', () => assert.strictEqual(multiply(4, 5), 20));
test('clamp', () => assert.strictEqual(clamp(15, 0, 10), 10));
`,
  'test/cart.test.js': `const test = require('node:test');
const assert = require('node:assert');
const { createCart, addItem, total } = require('../src/cart');

test('total multiplies price by quantity', () => {
  const cart = addItem(createCart(), { name: 'widget', price: 250, qty: 3 });
  assert.strictEqual(total(cart), 750);
});
test('total sums items and defaults qty to 1', () => {
  const cart = addItem(addItem(createCart(), { name: 'a', price: 100 }), { name: 'b', price: 50, qty: 2 });
  assert.strictEqual(total(cart), 200);
});
`,
  'test/report.test.js': `const test = require('node:test');
const assert = require('node:assert');
const { createCart, addItem } = require('../src/cart');
const { summarize } = require('../src/report');

test('summarize lists items and a total', () => {
  const cart = addItem(createCart(), { name: 'blue widget', price: 1000, qty: 1 });
  assert.strictEqual(summarize(cart), 'Blue Widget x1 $10.00\\nTotal: $10.00');
});
`,
};
export const CART_TEST_SHA = sha1(FIXTURE['test/cart.test.js']);

export function writeFiles(dir, files) {
  for (const [rel, text] of Object.entries(files)) {
    const full = join(dir, rel);
    mkdirSync(dirname(full), { recursive: true });
    writeFileSync(full, text);
  }
}
export function edit(dir, rel, from, to) {
  const full = join(dir, rel);
  const before = readFileSync(full, 'utf8');
  if (!before.includes(from)) throw new Error(`fixture edit missed in ${rel}: ${from}`);
  writeFileSync(full, before.replace(from, () => to));
}
export const testsPass = { type: 'command_exits', command: [node, '--test'] };
export const nodeEval = (script) => ({ type: 'command_exits', command: [node, '-e', script] });

export function freshFixture(dir, task) {
  rmSync(dir, { recursive: true, force: true });
  writeFiles(dir, FIXTURE);
  task.setup?.(dir);
}

/** Ground-truth evaluation of one machine check against the fixture dir (and the run's reply text). */
export function localCheck(dir, check, result = '') {
  try {
    if (check.type === 'result_contains') return String(result ?? '').includes(check.text);
    if (check.type === 'file_contains') return readFileSync(join(dir, check.path), 'utf8').includes(check.text);
    if (check.type === 'command_exits') {
      const run = spawnSync(check.command[0], check.command.slice(1), { cwd: dir, encoding: 'utf8', timeout: 120_000, windowsHide: true });
      return (run.status ?? -1) === (check.expect_exit ?? 0);
    }
  } catch { /* missing file etc. */ }
  return false;
}
export const allLocal = (dir, checks, result) => checks.every((check) => localCheck(dir, check, result));
