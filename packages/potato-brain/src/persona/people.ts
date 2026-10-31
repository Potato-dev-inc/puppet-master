export interface Person {
  id: string;
  name: string;
  lastAt: number;
}

/** Structural: anything shaped like a person node works, no import needed. */
export interface PersonBrief {
  name: string;
  habits: string[];
  pending: string[];
  jokes: string[];
  warmth: number;
  grudgeUntil: string;
  impression: string;
}

export function warmthBand(n: number): string {
  if (n >= 80) return "很暖";
  if (n >= 60) return "偏暖";
  if (n >= 40) return "平常";
  if (n >= 20) return "有點冷";
  return "在生氣";
}

/**
 * The part of a person node that goes in the tip: the two or three things
 * that make a reply feel like it was written by someone who knows you.
 * Never the whole file — that lives in the vault and costs cache.
 */
export function renderPersonBrief(p: PersonBrief, now: Date): string {
  const bits: string[] = [];

  const habits = p.habits.slice(-2);
  if (habits.length > 0) bits.push(`習慣 ${habits.join("；")}`);

  const pending = p.pending.slice(-2);
  if (pending.length > 0) bits.push(`還沒結的事 ${pending.join("；")}`);

  const joke = p.jokes[p.jokes.length - 1];
  if (joke) bits.push(`一起笑過「${joke}」`);

  const grudge = Date.parse(p.grudgeUntil);
  if (Number.isFinite(grudge) && grudge > now.getTime()) {
    bits.push("你還有點氣，沒完全消");
  } else {
    bits.push(`你現在對他${warmthBand(p.warmth)}`);
  }

  return bits.length === 0 ? "" : `（關於 ${p.name}：${bits.join("；")}）`;
}


/**
 * Compact on purpose: this sits at the tail of every request, so every token
 * here is a token that is never cached.
 */
export function renderPeople(
  people: Person[],
  currentId?: string,
  limit = 6,
): string {
  const recent = [...people].sort((a, b) => b.lastAt - a.lastAt).slice(0, limit);
  if (recent.length === 0) return "";

  const names = recent.map((person) => person.name).join("、");
  const current = recent.find((person) => person.id === currentId);
  const speaker = current ? `（現在：${current.name}）` : "";

  return `<people>多人頻道；[名字] 是說話的人，別把不同人接成同一句。${names}${speaker}</people>`;
}
