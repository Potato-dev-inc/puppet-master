// Benchmark task definitions (fixture-based, machine-verified) and task selection.
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { PONG } from './context.mjs';
import { CART_TEST_SHA, edit, nodeEval, testsPass, writeFiles } from './bench-fixture.mjs';

// A task: id, size (small|multi), prompt, checks (machine checks, run by PM and again locally),
// optional setup(dir) that seeds the fixture, solve(dir) = a reference solution used only by
// --dry-run to prove the checks are satisfiable (it returns the reply text for result checks),
// and an optional warm followup for small tasks.
export const TASKS = [
  {
    id: 'pong', size: 'small',
    prompt: PONG,
    checks: [{ type: 'result_contains', text: 'PONG' }],
    solve: () => 'PONG',
    warm: { task: 'Reply with only the word PING. Use no tools.', checks: [{ type: 'result_contains', text: 'PING' }], solve: () => 'PING' },
  },
  {
    id: 'create_file', size: 'small',
    prompt: 'Create a file named hello.txt in the current folder containing exactly the text REGRESSION-OK. Do nothing else.',
    checks: [{ type: 'file_contains', path: 'hello.txt', text: 'REGRESSION-OK' }],
    solve: (dir) => writeFiles(dir, { 'hello.txt': 'REGRESSION-OK' }),
    warm: {
      task: 'Create a file named warm.txt in the current folder containing exactly the text WARM-OK. Do nothing else.',
      checks: [{ type: 'file_contains', path: 'warm.txt', text: 'WARM-OK' }],
      solve: (dir) => writeFiles(dir, { 'warm.txt': 'WARM-OK' }),
    },
  },
  {
    id: 'rename_symbol', size: 'small',
    prompt: 'In src/math.js only, rename the function isEven to isEvenNumber (definition and export). Do not touch any other file.',
    checks: [
      { type: 'file_contains', path: 'src/math.js', text: 'isEvenNumber' },
      nodeEval("const m=require('./src/math');process.exit(typeof m.isEvenNumber==='function'&&!('isEven' in m)&&m.isEvenNumber(4)?0:1)"),
      testsPass,
    ],
    solve: (dir) => { const f = join(dir, 'src/math.js'); writeFileSync(f, readFileSync(f, 'utf8').replace(/\bisEven\b/g, 'isEvenNumber')); },
    warm: { task: 'Which file defines the function multiply? Reply with only its relative path.', checks: [{ type: 'result_contains', text: 'math.js' }], solve: () => 'src/math.js' },
  },
  {
    id: 'read_question', size: 'small',
    prompt: 'Read src/format.js and tell me what string formatPrice(1999) returns. Reply with only that string. Do not modify any file.',
    checks: [{ type: 'result_contains', text: '$19.99' }],
    solve: () => '$19.99',
    warm: { task: 'Now read src/math.js: what does clamp(15, 0, 10) return? Reply with only the number.', checks: [{ type: 'result_contains', text: '10' }], solve: () => '10' },
  },
  {
    id: 'rename_fn', size: 'multi',
    prompt: 'Rename the function multiply to mul everywhere in this project: its definition and export in src/math.js, and every use in src/cart.js, src/report.js and test/math.test.js. Behaviour must not change; run `node --test` to confirm it still passes.',
    checks: [
      { type: 'file_contains', path: 'src/math.js', text: 'mul' },
      nodeEval("const fs=require('fs');const bad=['src/math.js','src/cart.js','src/report.js','test/math.test.js'].filter((f)=>/\\bmultiply\\b/.test(fs.readFileSync(f,'utf8')));process.exit(bad.length?1:0)"),
      testsPass,
    ],
    solve: (dir) => { for (const f of ['src/math.js', 'src/cart.js', 'src/report.js', 'test/math.test.js']) { const p = join(dir, f); writeFileSync(p, readFileSync(p, 'utf8').replace(/\bmultiply\b/g, 'mul')); } },
  },
  {
    id: 'add_field', size: 'multi',
    prompt: 'Cart items should accept an optional string field `sku` (default null) that addItem stores on the item. Implement it in src/cart.js and add a test to test/cart.test.js that adds an item with a sku and asserts it is stored. Run `node --test` to confirm everything passes.',
    checks: [
      { type: 'file_contains', path: 'src/cart.js', text: 'sku' },
      { type: 'file_contains', path: 'test/cart.test.js', text: 'sku' },
      nodeEval("const {createCart,addItem}=require('./src/cart');const c=addItem(createCart(),{name:'a',price:1,sku:'X1'});const d=addItem(createCart(),{name:'b',price:1});process.exit(c.items[0].sku==='X1'&&d.items[0].sku===null?0:1)"),
      testsPass,
    ],
    solve: (dir) => {
      edit(dir, 'src/cart.js', 'qty: item.qty ?? 1 }', 'qty: item.qty ?? 1, sku: item.sku ?? null }');
      const f = join(dir, 'test/cart.test.js');
      writeFileSync(f, `${readFileSync(f, 'utf8')}\ntest('sku is stored', () => {\n  const cart = addItem(createCart(), { name: 'a', price: 1, sku: 'X1' });\n  assert.strictEqual(cart.items[0].sku, 'X1');\n});\n`);
    },
  },
  {
    id: 'fix_bug', size: 'multi',
    prompt: '`node --test` is failing. Find the bug in the source code under src/ and fix it. Do not edit anything under test/. Run `node --test` to confirm it passes.',
    setup: (dir) => edit(dir, 'src/cart.js', 'add(sum, multiply(item.price, item.qty))', 'add(sum, item.price)'),
    checks: [
      testsPass,
      nodeEval(`const c=require('crypto');const fs=require('fs');process.exit(c.createHash('sha1').update(fs.readFileSync('test/cart.test.js','utf8')).digest('hex')==='${CART_TEST_SHA}'?0:1)`),
    ],
    solve: (dir) => edit(dir, 'src/cart.js', 'add(sum, item.price)', 'add(sum, multiply(item.price, item.qty))'),
  },
  {
    id: 'add_module', size: 'multi',
    prompt: 'Add src/discount.js exporting applyDiscount(cents, percent): it returns cents reduced by percent (percent clamped to 0..100 using clamp from src/math.js), rounded to a whole number of cents. Wire it into src/report.js: summarize(cart, options = {}) must apply options.discountPercent (default 0) to the Total line only. Add test/discount.test.js covering applyDiscount. Run `node --test` to confirm everything passes.',
    checks: [
      { type: 'file_contains', path: 'src/discount.js', text: 'applyDiscount' },
      { type: 'file_contains', path: 'src/report.js', text: 'applyDiscount' },
      { type: 'file_contains', path: 'test/discount.test.js', text: 'applyDiscount' },
      nodeEval("const {applyDiscount}=require('./src/discount');const {createCart,addItem}=require('./src/cart');const {summarize}=require('./src/report');const c=addItem(createCart(),{name:'widget',price:1000,qty:1});process.exit(applyDiscount(1000,10)===900&&applyDiscount(1000,150)===0&&summarize(c,{discountPercent:10}).includes('Total: $9.00')&&summarize(c).includes('Total: $10.00')?0:1)"),
      testsPass,
    ],
    solve: (dir) => {
      writeFiles(dir, {
        'src/discount.js': "const { clamp } = require('./math');\nfunction applyDiscount(cents, percent) { return Math.round(cents * (1 - clamp(percent, 0, 100) / 100)); }\nmodule.exports = { applyDiscount };\n",
        'test/discount.test.js': "const test = require('node:test');\nconst assert = require('node:assert');\nconst { applyDiscount } = require('../src/discount');\n\ntest('applyDiscount', () => assert.strictEqual(applyDiscount(1000, 10), 900));\n",
      });
      edit(dir, 'src/report.js', "const { total } = require('./cart');", "const { total } = require('./cart');\nconst { applyDiscount } = require('./discount');");
      edit(dir, 'src/report.js', 'function summarize(cart) {', 'function summarize(cart, options = {}) {');
      edit(dir, 'src/report.js', 'formatPrice(total(cart))', 'formatPrice(applyDiscount(total(cart), options.discountPercent ?? 0))');
    },
  },
  {
    id: 'dedupe', size: 'multi',
    prompt: 'src/receipt.js and src/invoice.js each contain an identical private copy of the function formatLine. Move it into a new shared module src/line.js (exporting formatLine) and import it from both files. Keep the exported receiptLine and invoiceLine behaviour unchanged and do not edit tests. Run `node --test` to confirm it passes.',
    setup: (dir) => {
      const line = "function formatLine(item) { return `${titleCase(item.name)} x${item.qty} ${formatPrice(multiply(item.price, item.qty))}`; }";
      const head = "const { formatPrice, titleCase } = require('./format');\nconst { multiply } = require('./math');\n\n";
      writeFiles(dir, {
        'src/receipt.js': `${head}${line}\nfunction receiptLine(item) { return \`RECEIPT \${formatLine(item)}\`; }\nmodule.exports = { receiptLine };\n`,
        'src/invoice.js': `${head}${line}\nfunction invoiceLine(item) { return \`INVOICE \${formatLine(item)}\`; }\nmodule.exports = { invoiceLine };\n`,
        'test/lines.test.js': "const test = require('node:test');\nconst assert = require('node:assert');\nconst { receiptLine } = require('../src/receipt');\nconst { invoiceLine } = require('../src/invoice');\n\nconst item = { name: 'blue widget', price: 500, qty: 2 };\ntest('receiptLine', () => assert.strictEqual(receiptLine(item), 'RECEIPT Blue Widget x2 $10.00'));\ntest('invoiceLine', () => assert.strictEqual(invoiceLine(item), 'INVOICE Blue Widget x2 $10.00'));\n",
      });
    },
    checks: [
      { type: 'file_contains', path: 'src/line.js', text: 'formatLine' },
      nodeEval("const fs=require('fs');const n=fs.readdirSync('src').filter((f)=>f.endsWith('.js')).map((f)=>(fs.readFileSync('src/'+f,'utf8').match(/function formatLine/g)||[]).length).reduce((a,b)=>a+b,0);process.exit(n===1?0:1)"),
      testsPass,
    ],
    solve: (dir) => {
      const lineSrc = "const { formatPrice, titleCase } = require('./format');\nconst { multiply } = require('./math');\n\nfunction formatLine(item) { return `${titleCase(item.name)} x${item.qty} ${formatPrice(multiply(item.price, item.qty))}`; }\nmodule.exports = { formatLine };\n";
      writeFiles(dir, { 'src/line.js': lineSrc });
      for (const [file, fn, tag] of [['src/receipt.js', 'receiptLine', 'RECEIPT'], ['src/invoice.js', 'invoiceLine', 'INVOICE']]) {
        writeFiles(dir, { [file]: `const { formatLine } = require('./line');\n\nfunction ${fn}(item) { return \`${tag} \${formatLine(item)}\`; }\nmodule.exports = { ${fn} };\n` });
      }
    },
  },
];

export const TASK_GROUPS = { small: TASKS.filter((t) => t.size === 'small').map((t) => t.id), multi: TASKS.filter((t) => t.size === 'multi').map((t) => t.id) };
