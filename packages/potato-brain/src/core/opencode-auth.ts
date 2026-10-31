import { existsSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

function candidates(): string[] {
  const home = homedir();
  const list = [
    process.env.OPENCODE_AUTH_FILE,
    process.env.XDG_DATA_HOME ? join(process.env.XDG_DATA_HOME, "opencode", "auth.json") : undefined,
    join(home, ".local", "share", "opencode", "auth.json"),
    join(home, ".config", "opencode", "auth.json"),
    process.env.APPDATA ? join(process.env.APPDATA, "opencode", "auth.json") : undefined,
  ];
  return list.filter((p): p is string => typeof p === "string" && p !== "");
}

/**
 * Reuse the API key opencode already stored from `/connect`, so a Go
 * subscription works here without a second copy of the secret.
 * Returns undefined on any problem; never throws, never logs the key.
 */
export function readAuthKey(providerId: string): string | undefined {
  for (const path of candidates()) {
    if (!existsSync(path)) continue;
    try {
      const raw = JSON.parse(readFileSync(path, "utf8")) as Record<string, unknown>;
      const entry = raw[providerId];
      if (!entry || typeof entry !== "object") continue;

      const e = entry as Record<string, unknown>;
      const key = e["key"];
      if (typeof key === "string" && key !== "") return key;

      const access = e["access"];
      if (typeof access === "string" && access !== "") return access;
    } catch {
      continue;
    }
  }
  return undefined;
}
