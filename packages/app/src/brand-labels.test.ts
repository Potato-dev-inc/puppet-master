// UX-4: "Claude Code" is Anthropic's product name and must not appear in any text a person reads.
// Scans every .tsx file under src for string literals and JSX text (comments are not AST literals,
// so they are skipped). Test files are excluded. Labels say "Claude" (terminal) or "Claude Agent".
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';

const SRC = fileURLToPath(new URL('.', import.meta.url));
const FORBIDDEN = 'Claude Code';

function tsxFiles(dir: string): string[] {
  const out: string[] = [];
  for (const name of readdirSync(dir)) {
    const full = join(dir, name);
    if (statSync(full).isDirectory()) {
      if (name === 'node_modules' || name === '__tests__') continue;
      out.push(...tsxFiles(full));
    } else if (name.endsWith('.tsx') && !/\.test\.tsx$/.test(name)) {
      out.push(full);
    }
  }
  return out;
}

function renderedHits(file: string): string[] {
  const text = readFileSync(file, 'utf8');
  const sf = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const hits: string[] = [];
  const visit = (node: ts.Node): void => {
    if (
      ts.isStringLiteral(node) ||
      ts.isNoSubstitutionTemplateLiteral(node) ||
      ts.isJsxText(node) ||
      ts.isTemplateHead(node) ||
      ts.isTemplateMiddle(node) ||
      ts.isTemplateTail(node)
    ) {
      if (node.text.includes(FORBIDDEN)) {
        const line = sf.getLineAndCharacterOfPosition(node.getStart(sf)).line + 1;
        hits.push(`${relative(SRC, file)}:${line}: ${node.text.trim().slice(0, 80)}`);
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(sf);
  return hits;
}

describe('brand: no "Claude Code" in rendered text', () => {
  it('scans src/**/*.tsx and finds no "Claude Code" in string literals or JSX text', () => {
    const files = tsxFiles(SRC);
    expect(files.length).toBeGreaterThan(50);
    const hits = files.flatMap(renderedHits);
    expect(hits).toEqual([]);
  });
});
