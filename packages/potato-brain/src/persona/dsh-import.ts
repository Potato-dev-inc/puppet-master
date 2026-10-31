import { existsSync, mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { basename, join } from "node:path";
import { packsRoot } from "./packs.ts";

/**
 * DSH plugins keep the persona in a YAML block scalar, either under
 * `persona: |-` (deployment patch) or `text: |-` (agent preset persona row).
 * We do not need a YAML parser to read a block scalar.
 */
export function extractPersona(yaml: string): string | null {
  const head = /^[ \t]*(?:text|persona):[ \t]*\|[-+]?[ \t]*$/m.exec(yaml);
  if (!head) return null;

  const rest = yaml.slice(head.index + head[0].length).split(/\r?\n/);
  const block: string[] = [];
  let indent = -1;

  for (const line of rest) {
    if (line.trim() === "") {
      block.push("");
      continue;
    }

    const lead = line.length - line.trimStart().length;
    if (indent === -1) {
      if (lead === 0) break;
      indent = lead;
    }
    if (lead < indent) break;

    block.push(line.slice(indent));
  }

  const text = block.join("\n").trim();
  return text === "" ? null : text;
}

export interface ImportResult {
  pack: string;
  path: string;
  chars: number;
  source: string;
}

function yamlFiles(dir: string, depth = 0): string[] {
  if (depth > 4 || !existsSync(dir)) return [];
  const out: string[] = [];

  for (const name of readdirSync(dir)) {
    if (name === "node_modules" || name.startsWith(".")) continue;
    const abs = join(dir, name);

    let isDir = false;
    try {
      isDir = statSync(abs).isDirectory();
    } catch {
      continue;
    }

    if (isDir) out.push(...yamlFiles(abs, depth + 1));
    else if (name.endsWith(".yml") || name.endsWith(".yaml")) out.push(abs);
  }

  return out;
}

function pickSource(files: string[]): { file: string; text: string } | null {
  const preferred = [
    ...files.filter((f) => f.replace(/\\/g, "/").includes("agent-presets/")),
    ...files.filter((f) => basename(f) === "cordis.patch.yml"),
    ...files,
  ];

  for (const file of preferred) {
    try {
      const text = readFileSync(file, "utf8");
      const persona = extractPersona(text);
      if (persona && persona.length > 80) return { file, text: persona };
    } catch {
      continue;
    }
  }

  return null;
}

/**
 * Turn a DSH plugin directory (or an already-extracted .md) into a persona pack
 * under `vault/persona/packs/<slug>/core.md`. The original stays read-only.
 */
export function importDshPersona(args: {
  sourceDir: string;
  personaDir: string;
  slug?: string;
  credit?: string;
}): ImportResult | null {
  const slug =
    (args.slug ?? "").trim().replace(/[^\w.-]+/g, "-").replace(/^-+|-+$/g, "") ||
    basename(args.sourceDir).replace(/[^\w.-]+/g, "-") ||
    "pack";

  let body: string | null = null;
  let source = "";

  const single = join(args.sourceDir, "core.md");
  if (existsSync(single)) {
    body = readFileSync(single, "utf8").trim();
    source = "core.md";
  } else {
    const found = pickSource(yamlFiles(args.sourceDir));
    if (found) {
      body = found.text;
      source = basename(found.file);
    }
  }

  if (!body || body.trim() === "") return null;

  const header = [
    `<!-- imported from ${args.credit ?? args.sourceDir} (${source}) -->`,
    "",
    body.trim(),
    "",
  ].join("\n");

  const dir = join(packsRoot(args.personaDir), slug);
  mkdirSync(dir, { recursive: true });
  const path = join(dir, "core.md");
  writeFileSync(path, header, "utf8");

  return { pack: slug, path, chars: header.length, source };
}
