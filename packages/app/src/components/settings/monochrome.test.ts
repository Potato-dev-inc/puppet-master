// @vitest-environment node
// Settings is monochrome (mockups/ui-proposal/STYLE.md): no hue classes in any settings source file.
import { readdirSync, readFileSync, statSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const here = path.dirname(fileURLToPath(import.meta.url));
const HUE = /\b(?:text|bg|border|ring|fill|stroke|from|to|via|divide|outline|accent|decoration)-(?:red|green|emerald|amber|yellow|orange|blue|sky|indigo|purple|pink|rose|lime|teal|cyan|violet|fuchsia|slate)-\d{2,3}/;

function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const full = path.join(dir, name);
    if (statSync(full).isDirectory()) return sources(full);
    return /\.(tsx?|css)$/.test(name) && !/\.test\./.test(name) ? [full] : [];
  });
}

describe('settings stays monochrome', () => {
  const files = [...sources(here), path.join(here, '../MobilePairingPanel.tsx'), path.join(here, '../SettingsPanel.tsx'), path.join(here, '../../styles/settings.css')];

  it.each(files.map((f) => [path.relative(path.join(here, '../..'), f), f]))('%s has no hue classes', (_name, file) => {
    const text = readFileSync(file, 'utf8');
    const hit = text.match(HUE);
    expect(hit, `${_name}: ${hit?.[0]}`).toBeNull();
  });

  it('has no red "Planned" badge component left', () => {
    const ui = readFileSync(path.join(here, 'settings-ui.tsx'), 'utf8');
    expect(ui).not.toMatch(/PlannedBadge|Not implemented/);
  });
});
