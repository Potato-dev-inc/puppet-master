// @vitest-environment node
// Checks on the files the relay serves: the built page and the sources. These are the rules of SPEC-n32 3.9
// that a unit test cannot see: no inputs, no markup injection, no storage, no third-party loads.
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { describe, expect, it } from 'vitest';

const ROOT = join(__dirname, '..');
const SRC = join(ROOT, 'src');
const DIST = join(ROOT, 'dist');

function filesIn(dir: string): string[] {
  const out: string[] = [];
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) {
      out.push(...filesIn(path));
    } else {
      out.push(path);
    }
  }
  return out;
}

const sourceFiles = filesIn(SRC);
const sourceText = sourceFiles.map((file) => ({ name: relative(ROOT, file), text: readFileSync(file, 'utf8') }));

describe('built page (dist/)', () => {
  const html = readFileSync(join(DIST, 'viewer.html'), 'utf8');

  it('has the three files the relay serves', () => {
    for (const file of ['viewer.html', 'viewer.js', 'viewer.css']) {
      expect(statSync(join(DIST, file)).size).toBeGreaterThan(0);
    }
  });

  it('has no input, textarea, form, select or editable element', () => {
    expect(html).not.toMatch(/<(input|textarea|form|select)\b/i);
    expect(html).not.toMatch(/contenteditable/i);
  });

  it('has no inline script or style, and loads only the same-origin files', () => {
    expect(html).not.toMatch(/<script(?![^>]*\bsrc=)/i);
    expect(html).not.toMatch(/<style\b/i);
    expect(html).not.toMatch(/\sstyle=/i);
    expect(html).toContain('href="viewer.css"');
    expect(html).toContain('src="viewer.js"');
  });

  it('has no link to another place', () => {
    expect(html).not.toMatch(/https?:\/\//i);
  });

  it('the script has no markup injection, eval, storage or cookie access', () => {
    const js = readFileSync(join(DIST, 'viewer.js'), 'utf8');
    expect(js).not.toMatch(/innerHTML|outerHTML|insertAdjacentHTML/);
    expect(js).not.toMatch(/\beval\b/);
    expect(js).not.toMatch(/new Function/);
    expect(js).not.toMatch(/localStorage|sessionStorage|document\.cookie/);
  });
});

describe('sources (src/)', () => {
  it('never uses innerHTML or its relatives', () => {
    for (const file of sourceText) {
      expect(file.text, file.name).not.toMatch(/innerHTML|outerHTML|insertAdjacentHTML/);
    }
  });

  it('never calls eval or new Function', () => {
    for (const file of sourceText) {
      expect(file.text, file.name).not.toMatch(/\beval\b/);
      expect(file.text, file.name).not.toMatch(/new Function/);
    }
  });

  it('never touches local storage or cookies', () => {
    for (const file of sourceText) {
      expect(file.text, file.name).not.toMatch(/localStorage|sessionStorage|document\.cookie/);
    }
  });

  it('names no web address (the page talks only to its own origin)', () => {
    for (const file of sourceText) {
      expect(file.text, file.name).not.toMatch(/https?:\/\//);
    }
  });

  it('has no input or editable markup in the page source', () => {
    const html = readFileSync(join(SRC, 'viewer.html'), 'utf8');
    expect(html).not.toMatch(/<(input|textarea|form|select)\b/i);
    expect(html).not.toMatch(/contenteditable/i);
  });
});
