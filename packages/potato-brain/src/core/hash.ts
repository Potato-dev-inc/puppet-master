import { createHash } from "node:crypto";

export function sha(input: string): string {
  return createHash("sha256").update(input, "utf8").digest("hex").slice(0, 16);
}

/** Hash an ordered list of blocks by stable id + content. */
export function hashBlocks(blocks: ReadonlyArray<{ id: string; content: string }>): string {
  const canonical = blocks.map((b) => `${b.id}\u0000${b.content}`).join("\u0001");
  return sha(canonical);
}

/**
 * Stable, collision-resistant-enough seed from any string.
 * Used for deterministic per-message choices (kaomoji, voice lottery).
 */
export function seedOf(input: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < input.length; i++) {
    h ^= input.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h >>> 0;
}

/** Deterministic float in [0, 1) from a seed. */
export function rand01(seed: number): number {
  let x = seed >>> 0;
  x ^= x << 13;
  x >>>= 0;
  x ^= x >>> 17;
  x ^= x << 5;
  x >>>= 0;
  return x / 0x100000000;
}
