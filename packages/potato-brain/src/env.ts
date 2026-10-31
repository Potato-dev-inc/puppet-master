import { existsSync, readFileSync } from "node:fs";

function applyEnv(raw: string, override: boolean): void {
  for (const line of raw.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (trimmed === "" || trimmed.startsWith("#")) continue;
    const eq = trimmed.indexOf("=");
    if (eq <= 0) continue;

    const key = trimmed.slice(0, eq).trim();
    let value = trimmed.slice(eq + 1).trim();
    if (
      (value.startsWith('"') && value.endsWith('"') && value.length >= 2) ||
      (value.startsWith("'") && value.endsWith("'") && value.length >= 2)
    ) {
      value = value.slice(1, -1);
    }
    if (override || process.env[key] === undefined) process.env[key] = value;
  }
}

/**
 * Minimal .env loader. No dependency, no override: variables already set in
 * the real environment always win.
 */
export function loadEnv(file = ".env"): void {
  if (!existsSync(file)) return;
  let raw: string;
  try {
    raw = readFileSync(file, "utf8");
  } catch {
    return;
  }
  applyEnv(raw, false);
}

let lastHash = "";

/**
 * Re-read .env and let the file win, so editing settings takes effect without
 * a restart. Cheap: no-ops unless the bytes actually changed.
 */
export function reloadEnv(file = ".env"): boolean {
  if (!existsSync(file)) return false;

  let raw: string;
  try {
    raw = readFileSync(file, "utf8");
  } catch {
    return false;
  }

  let hash = 0;
  for (let i = 0; i < raw.length; i++) hash = (Math.imul(hash, 31) + raw.charCodeAt(i)) | 0;
  const key = String(hash);
  if (key === lastHash) return false;

  lastHash = key;
  applyEnv(raw, true);
  return true;
}

/** Empty or garbage values fall back instead of becoming 0. */
export function envNumber(name: string, fallback: number, min = 0): number {
  const raw = (process.env[name] ?? "").trim();
  if (raw === "") return fallback;
  const value = Number(raw);
  if (!Number.isFinite(value) || value < min) return fallback;
  return value;
}

export function envFlag(name: string, fallback = false): boolean {
  const raw = (process.env[name] ?? "").trim().toLowerCase();
  if (raw === "") return fallback;
  return raw === "1" || raw === "true" || raw === "yes" || raw === "on";
}
