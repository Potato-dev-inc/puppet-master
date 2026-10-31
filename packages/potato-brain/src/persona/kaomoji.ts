import { rand01, seedOf } from "../core/hash.ts";

export const FEELS = [
  "happy",
  "proud",
  "shy",
  "sleepy",
  "hungry",
  "angry",
  "sad",
  "confused",
  "clingy",
] as const;

export type Feel = (typeof FEELS)[number];

const TABLE: Record<Feel, readonly string[]> = {
  happy: ["(＾▽＾)", "ヾ(＾∇＾)"],
  proud: ["(๑•̀ㅂ•́)و✧"],
  shy: ["(〃▽〃)", "(⁄ ⁄>⁄ ▽ ⁄<⁄ ⁄)"],
  sleepy: ["(￣ω￣)", "(－ω－) zzZ", "(´-ω-`)"],
  hungry: ["(๑´ڡ`๑)", "(｡･ω･｡)"],
  angry: ["(▼皿▼#)", "(｀ω´)", "(╬ Ò ‸ Ó)"],
  sad: ["(；ω；)", "(╥﹏╥)", "QwQ"],
  confused: ["(・ω・?)", "(。_。?)"],
  clingy: ["(´･ω･`)", "(つω`｡)", "ﾉ(｀ω´ﾉ)"],
};

export function isFeel(x: unknown): x is Feel {
  return typeof x === "string" && (FEELS as readonly string[]).includes(x);
}

/**
 * Deterministic per message: same feel + same seed -> same kaomoji.
 * The model picks the emotion; the table picks the face.
 */
export function pickKaomoji(feel: Feel, seed: number): string {
  const list = TABLE[feel];
  const i = Math.floor(rand01(seed ^ seedOf(feel)) * list.length);
  return list[i] ?? list[0] ?? "";
}
