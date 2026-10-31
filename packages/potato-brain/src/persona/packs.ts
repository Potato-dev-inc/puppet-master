import { existsSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

export function packsRoot(personaDir: string): string {
  return join(personaDir, "packs");
}

/** Available packs = subdirectories of persona/packs that contain a core.md. */
export function listPacks(personaDir: string): string[] {
  const root = packsRoot(personaDir);
  if (!existsSync(root)) return [];

  try {
    return readdirSync(root)
      .filter((name) => {
        const abs = join(root, name);
        return statSync(abs).isDirectory() && existsSync(join(abs, "core.md"));
      })
      .sort();
  } catch {
    return [];
  }
}

/** Unknown or empty pack falls back to the built-in persona, never to nothing. */
export function resolvePersonaDir(personaDir: string, pack?: string): string {
  const name = (pack ?? "").trim();
  if (name === "" || name === "default") return personaDir;

  const abs = join(packsRoot(personaDir), name);
  return existsSync(join(abs, "core.md")) ? abs : personaDir;
}
