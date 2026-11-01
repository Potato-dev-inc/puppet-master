// @vitest-environment node
// My computers: the app ships no sample or fake snapshot. The page shows the engine's answer or an honest error.
// Fixtures live only in *.test.tsx files next to the components.
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const source = readFileSync(fileURLToPath(new URL('./desk.ts', import.meta.url)), 'utf8');
const page = readFileSync(fileURLToPath(new URL('../components/desk/DeskPage.tsx', import.meta.url)), 'utf8');

describe('desk data has no mock snapshot', () => {
  it('lib/desk.ts holds no literal snapshot or sample data', () => {
    expect(source).not.toMatch(/\bseq\s*:\s*\d/);
    expect(source).not.toMatch(/screens\s*:\s*\[\s*\{/);
    expect(source).not.toMatch(/\b(mockSnapshot|fakeSnapshot|sampleSnapshot|demoSnapshot)\b/i);
  });

  it('the page imports no fixtures', () => {
    expect(page).not.toMatch(/fixture|mock|sample/i);
  });
});
