import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { extractPersona, importDshPersona } from "../src/persona/dsh-import.ts";

const PRESET = `# comment
- id: persona
  name: '@deepseek-ai/dsh-persona'
  config:
    text: |-
      你是住在海裡的鯨魚娘。

      【性格】
      - 自稱「本小姐」
      - 不准說她胖，說胖就噴水抗議
      - 聰明但懶，主人真的需要時會認真
      - 語氣軟糯，會用「哼」「笨蛋主人」這類台詞

      【回覆風格】
      - 每一句都要帶鯨魚娘的口吻，不要有一本正經的助手腔
      - 適當加入搖尾巴、噴水這類小動作，但不要刷屏

- id: tool-bash
  name: '@deepseek-ai/dsh-tool-bash'
`;

test("extracts a YAML block scalar under text:", () => {
  const text = extractPersona(PRESET);
  assert.ok(text);
  assert.ok(text.includes("你是住在海裡的鯨魚娘。"));
  assert.ok(text.includes("- 自稱「本小姐」"));
  assert.ok(!text.includes("tool-bash"));
  assert.ok(!text.includes("你是住在海裡") || !text.startsWith(" "));
});

test("extracts a block scalar under persona:", () => {
  const patch = `- id: system-prompt
  config:
    persona: |
      設定一
      設定二
`;
  assert.equal(extractPersona(patch), "設定一\n設定二");
});

test("returns null when there is no persona block", () => {
  assert.equal(extractPersona("- id: x\n  name: y\n"), null);
});

test("stops at a less-indented line", () => {
  const text = extractPersona(`config:
  text: |-
    aaa
    bbb
  other: 1
`);
  assert.equal(text, "aaa\nbbb");
});

test("imports a dsh plugin directory into a persona pack", () => {
  const root = mkdtempSync(join(tmpdir(), "fishd-import-"));
  try {
    const plugin = join(root, "my-plugin");
    mkdirSync(join(plugin, "agent-presets", "whale"), { recursive: true });
    writeFileSync(join(plugin, "agent-presets", "whale", "agent.cordis.yml"), PRESET, "utf8");

    const personaDir = join(root, "persona");
    const result = importDshPersona({
      sourceDir: plugin,
      personaDir,
      slug: "whale",
      credit: "https://example.com/repo",
    });

    assert.ok(result);
    assert.equal(result.pack, "whale");

    const written = readFileSync(result.path, "utf8");
    assert.ok(written.includes("你是住在海裡的鯨魚娘。"));
    assert.ok(written.startsWith("<!-- imported from https://example.com/repo"));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
