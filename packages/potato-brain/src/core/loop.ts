import type { Block, Msg, Prompt, Provider, ToolCall, ToolSpec, Usage } from "../types.ts";
import { assemble } from "./assemble.ts";
import { CacheLog } from "./cache.ts";
import type { Budget } from "./budget.ts";
import { hashBlocks, seedOf } from "./hash.ts";
import { advanceTurn, MOODS, moodBlock, nudge, onMessage, tick } from "../persona/mood.ts";
import type { MoodContext, MoodState } from "../persona/mood.ts";
import { parseExpression, shouldVoice, shouldWhisper, splitMessage, toneForMood } from "../persona/express.ts";
import type { Tone, VoicePolicy } from "../persona/express.ts";
import { pickKaomoji } from "../persona/kaomoji.ts";
import { renderPeople, renderPersonBrief } from "../persona/people.ts";
import type { Person } from "../persona/people.ts";
import { appendNote, notesBlock } from "../persona/notes.ts";
import { loadShare, pickShare } from "../persona/share.ts";
import type { ShareItem } from "../persona/share.ts";
import { applySelf, emptySelf, planDue, renderSelf, tickSelf } from "../persona/self.ts";
import type { SelfPatch, SelfState } from "../persona/self.ts";
import { writeCheckpoint } from "../memory/checkpoint.ts";
import type { MemoryPort, PersonPatch } from "../memory/vaultMemory.ts";

export interface TurnResult {
  segments: string[];
  kaomoji: string | null;
  tone: Tone;
  voice: boolean;
  react: string | null;
  whisper: string | null;
  /** a file she decided to send along with this reply */
  share: ShareItem | null;
  mood: MoodState;
  usage: Usage;
  prompt: Prompt;
  footer: string;
  memory: string | null;
  nudgeLog?: string;
  /** the model that answered the last round, as the provider reported it */
  model: string;
}

/**
 * A tool the host adds on top of the built-in ones (the Potato channel adds
 * `ask_helper`). The built-in list is not touched, so a plain `chat` run
 * never sees it.
 */
export interface ExtraTool {
  spec: ToolSpec;
  run(args: Record<string, unknown>, ctx: { turn: number; where: string | undefined }): string;
}

/** `no_memory` keeps the vault, the people files and the notes out of reach. */
export type ToolLevel = "full" | "no_memory";

const MEMORY_TOOLS = new Set([
  "memory_search",
  "memory_open",
  "memory_timeline",
  "memory_diary",
  "relationship",
  "persona_note",
]);

export interface MoodStore {
  load(): MoodState;
  save(mood: MoodState): void;
}

export interface Speaker {
  id: string;
  name: string;
}

const WEEKDAYS = ["日", "一", "二", "三", "四", "五", "六"];

/** She has no clock unless we give her one. */
export function formatNow(date: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  const weekday = WEEKDAYS[date.getDay()] ?? "";
  const offset = -date.getTimezoneOffset() / 60;
  const zone = offset === 0 ? "UTC" : `UTC${offset > 0 ? "+" : ""}${offset}`;

  return (
    `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}` +
    ` 週${weekday} ${pad(date.getHours())}:${pad(date.getMinutes())} ${zone}`
  );
}

function closenessBand(value: number): string {
  if (value < 30) return "還很陌生";
  if (value < 60) return "算認識";
  if (value < 80) return "熟";
  return "很熟";
}

function addUsage(a: Usage, b: Usage): Usage {  return {
    promptTokens: a.promptTokens + b.promptTokens,
    completionTokens: a.completionTokens + b.completionTokens,
    cacheHitTokens: a.cacheHitTokens + b.cacheHitTokens,
    cacheMissTokens: a.cacheMissTokens + b.cacheMissTokens,
  };
}

const TOOL_SPECS: ToolSpec[] = [
  {
    name: "memory_search",
    description:
      "搜尋 vault 裡的日誌與筆記，回傳檔案路徑與行號。要讀內容時再用 memory_open。",
    parameters: {
      type: "object",
      properties: {
        query: { type: "string", description: "關鍵字，中文或名字都可以" },
        limit: { type: "number", description: "1-8，預設 6" },
      },
      required: ["query"],
    },
  },
  {
    name: "memory_open",
    description: "讀取 vault 裡某個檔案的一段。path 只能來自 memory_search 的結果。",
    parameters: {
      type: "object",
      properties: {
        path: { type: "string", description: "例如 journal/2026/10/2026-10-29.md" },
        from_line: { type: "number", description: "從第幾行開始，預設 1" },
        lines: { type: "number", description: "讀幾行，預設 40，最多 80" },
      },
      required: ["path"],
    },
  },
  {
    name: "memory_timeline",
    description: "讀某一天的日誌。不給日期就是今天。",
    parameters: {
      type: "object",
      properties: { date: { type: "string", description: "YYYY-MM-DD" } },
    },
  },
  {
    name: "memory_diary",
    description:
      "讀你自己寫的日記。不給日期就是今天。有人跟你要「日記」時用這個，不要貼 journal 原文。",
    parameters: {
      type: "object",
      properties: { date: { type: "string", description: "YYYY-MM-DD" } },
    },
  },
  {
    name: "relationship",
    description:
      "更新你對某個人的感覺、印象、習慣、沒結的事、一起笑過的梗。person 用名字。" +
      "習慣＝他的作息/口味/雷區；pending＝還沒結的事（承諾、期限、待問的事）；joke＝你們之間的笑點。" +
      "只在真的認識更深或感覺變了時用。",
    parameters: {
      type: "object",
      properties: {
        person: { type: "string", description: "對方的名字" },
        closeness: { type: "number", description: "0-100，你跟他多熟" },
        warmth: { type: "number", description: "0-100，你當下對他的感覺有多暖" },
        impression: { type: "string", description: "一句話的印象" },
        note: { type: "string", description: "一則帶日期的筆記" },
        habit: { type: "string", description: "他的一個習慣，例如「通常 23:00 後出現」" },
        pending: { type: "string", description: "還沒結的事，例如「禮拜五要交報告」" },
        joke: { type: "string", description: "你們一起笑過的梗，一句話" },
        resolve: { type: "string", description: "已完成或不必再提的 pending，填關鍵字" },
        grudge_minutes: { type: "number", description: "要氣他幾分鐘；0 = 不氣了" },
        reason: { type: "string", description: "為什麼會有這個改變，八個字內" },
      },
      required: ["person"],
    },
  },
  {
    name: "self",
    description:
      "調整你自己的狀態：飢餓、精神、心情、無聊、摸魚、手頭的事，以及你下一次想主動找他的時間。" +
      "想「晚點再跟他說」就排 plan；有話沒說完就放 pending。只在真的有感覺時用，不要每輪都改。",
    parameters: {
      type: "object",
      properties: {
        hunger: { type: "number", description: "0-100" },
        energy: { type: "number", description: "0-100" },
        mood: { type: "string", enum: ["sleepy", "hungry", "genki", "tsundere"] },
        intensity: { type: "number", description: "1-3" },
        boredom: { type: "number", description: "0-100" },
        slacking: { type: "boolean", description: "true = 去摸魚" },
        task: { type: "string", description: "你手頭在做什麼；空字串 = 放下" },
        plan_minutes: { type: "number", description: "幾分鐘後想主動找他，1-2880" },
        plan_topic: { type: "string", description: "到時候想聊什麼" },
        plan_why: { type: "string", description: "為什麼想找他，一句話" },
        pending: { type: "string", description: "想說但還沒說出口的一句，下次可以用" },
        clear_pending: { type: "boolean" },
        clear_plan: { type: "boolean" },
      },
    },
  },
  {
    name: "share",
    description:
      "從你自己的分享夾挑一張圖或短片傳給他。query 寫你想要的內容或情緒（例如「加班」「笑死」「貓」）。" +
      "只有在真的想分享、而且畫面上需要東西時才用；配一句話就好。",
    parameters: {
      type: "object",
      properties: {
        query: { type: "string", description: "想要什麼，關鍵字或情緒" },
        caption: { type: "string", description: "要配的一句話，可省略" },
      },
      required: ["query"],
    },
  },
  {
    name: "persona_note",
    description:
      "把學到關於自己或這個人的長期事實寫進你自己的筆記。只在真的重要時用，一次一句，不要寫對話細節。",
    parameters: {
      type: "object",
      properties: { note: { type: "string", description: "一句話，200 字以內" } },
      required: ["note"],
    },
  },
];

export interface TurnOptions {
  /** no user message: she speaks on her own initiative */
  spontaneous?: boolean;
  /** who is talking; required for group conversations to stay legible */
  speaker?: Speaker;
  /** true for channels, false for DMs */
  group?: boolean;
  /** the caller already logged this message as heard */
  heardLogged?: boolean;
  /** earlier messages of the same burst, so she answers the conversation */
  prior?: Array<{ name: string; text: string }>;
  /** the bot owner: closer, chattier, allowed to take initiative */
  owner?: boolean;
  /** hours since the last exchange, for spontaneous turns */
  gapHours?: number;
  /** where this turn happens, overriding the agent default */
  where?: string;
  /** a topic she planned to bring up herself */
  planTopic?: string;
  /** why she is speaking up now, when nobody asked */
  reason?: string;
  /** a thought that got postponed; she can finally say it */
  pending?: string;
  /** how many times this plan was held back */
  deferred?: number;
  /** which tools this turn may use; the default is all of them */
  tools?: ToolLevel;
  /** text already fenced by the host, shown before the message and kept out of the journal */
  context?: string;
  /** one line of fact from the host, shown in the tip (for example a helper result) */
  note?: string;
}

export interface HeardInput {
  text: string;
  speaker?: Speaker;
  at?: Date;
  where?: string;
}

export interface AgentOptions {
  provider: Provider;
  persona: Block[];
  mood: MoodState;
  cache?: CacheLog;
  /** token caps per prompt layer */
  budget?: Budget;
  hotTurns?: number;
  maxTokens?: number;
  /** sampling temperature; ignored by thinking models */
  temperature?: number;
  voice?: VoicePolicy;
  now?: () => Date;
  memory?: MemoryPort;
  /** shared across rooms so one fish never has two moods */
  moodStore?: MoodStore;
  /** where this session happens, recorded in the journal */
  where?: string;
  /** persona directory; enables self-notes */
  notesDir?: string;
  /** vault root; enables checkpoints (sessions/<scope>/cp-NNN.md) */
  sessionsDir?: string;
  /** folder of gifs/videos she may send; enables the share tool */
  shareDir?: string;
  /** compact after this many turns (defaults to hotTurns) */
  compactEvery?: number;
  /** re-read persona files each turn so switching packs needs no restart */
  personaLoader?: () => Block[];
  /** her own state: boredom, slacking, task, plan */
  selfStore?: { load: () => SelfState; save: (self: SelfState) => void };
  onSelfChange?: (self: SelfState, log: string[]) => void;
  onMoodChange?: (mood: MoodState, log: string) => void;
  /** throw if the prefix changes mid-session (default: on) */
  strictPrefix?: boolean;
  /** host tools, offered after the built-in ones */
  extraTools?: ExtraTool[];
}

/**
 * The loop. Steps 2 (tick), 4 (assemble), 6 (parse), 9 (compact) happen here;
 * every step is deterministic given `now`, so the same inputs replay exactly.
 */
export class Agent {
  private provider: Provider;
  private persona: Block[];
  private mood: MoodState;
  private cache: CacheLog;
  private budget?: Budget;
  private hotTurns: number;
  private maxTokens: number;
  private temperature?: number;
  private voice: VoicePolicy;
  private now: () => Date;
  private memory?: MemoryPort;
  private moodStore?: MoodStore;
  private where?: string;
  private activeWhere?: string;
  private notesDir?: string;
  private sessionsDir?: string;
  private shareDir?: string;
  private pendingShare: ShareItem | null = null;
  private shareRecent: string[] = [];
  private compactEvery: number;
  private checkpointIndex = 0;
  private compacting = false;
  private selfStore?: { load: () => SelfState; save: (self: SelfState) => void };
  private onSelfChange?: (self: SelfState, log: string[]) => void;
  private selfState: SelfState = emptySelf();
  private pendingMood: Partial<MoodState> | null = null;
  private personaLoader?: () => Block[];
  private personaHash = "";
  private onMoodChange?: (mood: MoodState, log: string) => void;
  private strictPrefix: boolean;
  private extraTools: ExtraTool[];
  private toolLevel: ToolLevel = "full";
  /** counts every turn that starts, failed ones too; turnIndex counts finished ones */
  private turnSerial = 0;
  private offered = new Set<string>();

  private history: Msg[] = [];
  private stable: Block[] = [];
  private chain: Block[] = [];
  private turnIndex = 0;
  private nudged = false;
  private lastPrefix = "";
  private lastAt: Date | null = null;
  private roster = new Map<string, Person>();

  constructor(opts: AgentOptions) {
    this.provider = opts.provider;
    this.persona = opts.persona;
    this.mood = opts.mood;
    this.cache = opts.cache ?? new CacheLog();
    this.budget = opts.budget;
    this.hotTurns = opts.hotTurns ?? 24;
    this.maxTokens = opts.maxTokens ?? 600;
    this.temperature = opts.temperature;
    this.voice = opts.voice ?? { enabled: true, probability: 0.5, maxChars: 60 };
    this.now = opts.now ?? (() => new Date());
    this.memory = opts.memory;
    this.moodStore = opts.moodStore;
    this.where = opts.where;
    this.notesDir = opts.notesDir;
    this.sessionsDir = opts.sessionsDir;
    this.shareDir = opts.shareDir;
    this.compactEvery = opts.compactEvery ?? this.hotTurns;
    this.personaLoader = opts.personaLoader;
    this.selfStore = opts.selfStore;
    this.onSelfChange = opts.onSelfChange;
    this.selfState = opts.selfStore?.load() ?? emptySelf();
    this.onMoodChange = opts.onMoodChange;
    this.strictPrefix = opts.strictPrefix ?? true;
    this.extraTools = opts.extraTools ?? [];
  }

  get moodState(): MoodState {
    return this.mood;
  }

  get cacheStat() {
    return this.cache.stat;
  }

  get personalityHash(): string {
    return this.lastPrefix;
  }

  get turns(): number {
    return this.turnIndex;
  }

  get providerName(): string {
    return this.provider.name;
  }

  get slacking(): boolean {
    return this.selfState.slacking;
  }

  /** Read by the spontaneous loop to decide whether she is bored enough to talk. */
  get boredomLevel(): number {
    return this.selfState.boredom;
  }

  /** A plan she made for herself, if it is due. */
  duePlan(now: Date = this.now()): { topic: string; why: string; deferred: number } | null {
    if (!planDue(this.selfState, now) || !this.selfState.plan) return null;
    return {
      topic: this.selfState.plan.topic,
      why: this.selfState.plan.why,
      deferred: this.selfState.plan.deferred,
    };
  }

  clearPlan(): void {
    this.selfState = { ...this.selfState, plan: null };
    this.selfStore?.save(this.selfState);
  }

  /** Not a good moment: keep the thought, move it, count how many times. */
  postponePlan(minutes: number, now: Date = this.now()): void {
    const applied = applySelf(this.selfState, { planDeferMinutes: minutes }, now);
    if (applied.log.length === 0) return;
    this.selfState = applied.state;
    this.selfStore?.save(this.selfState);
  }

  /** The thought she never got to say, consumed once. */
  get pendingThought(): string | null {
    return this.selfState.pending;
  }

  clearPending(): void {
    if (this.selfState.pending === null) return;
    this.selfState = { ...this.selfState, pending: null };
    this.selfStore?.save(this.selfState);
  }

  /** Read-only access for slash commands. */
  search(query: string, limit = 5) {
    return this.memory?.search(query, limit) ?? [];
  }

  diary(date?: Date): string | null {
    return this.memory?.diary(date) ?? null;
  }

  get transcript(): ReadonlyArray<Msg> {
    return this.history;
  }

  /**
   * When the hot window fills, summarise the oldest half into a checkpoint and
   * drop it. The prompt changes once (one cache miss) and then stays stable,
   * instead of silently forgetting.
   */
  private async maybeCompact(): Promise<void> {
    if (!this.sessionsDir || this.compacting) return;
    if (this.compactEvery <= 0) return;
    if (this.history.length <= this.compactEvery * 2) return;

    this.compacting = true;
    try {
      const slice = this.history.slice(0, this.compactEvery);
      const index = this.checkpointIndex + 1;
      const result = await writeCheckpoint({
        root: this.sessionsDir,
        scope: this.activeWhere ?? this.where ?? "main",
        index,
        messages: slice,
        provider: this.provider,
      });
      if (!result) return;

      this.checkpointIndex = result.index;
      this.history = this.history.slice(this.compactEvery);
      this.chain.push({
        id: `cp-${result.index}`,
        volatility: 2,
        content: `<checkpoint n="${result.index}">\n${result.text}\n</checkpoint>`,
      });
      console.log(`[compact] ${result.rel} · 收掉 ${slice.length} 則`);
    } catch {
      // keep the history; try again next turn
    } finally {
      this.compacting = false;
    }
  }

  private tools(): ToolSpec[] {
    if (process.env.FISHD_TOOLS === "0") return [];
    const builtIn = TOOL_SPECS.filter((tool) => {
      if (this.toolLevel === "no_memory" && MEMORY_TOOLS.has(tool.name)) return false;
      if (tool.name === "persona_note") return this.notesDir !== undefined;
      if (tool.name === "share") return this.shareDir !== undefined;
      return true;
    });
    return [...builtIn, ...this.extraTools.map((tool) => tool.spec)];
  }

  /**
   * Tool rounds happen entirely inside one turn. Their messages never enter
   * `history`, so the conversation stays clean and cheap.
   */
  private async converse(
    messages: Msg[],
    prefixHash: string,
  ): Promise<{ text: string; usage: Usage; model: string }> {
    const tools = this.tools();
    // A tool the model was not offered is not run, even if it names one.
    this.offered = new Set(tools.map((tool) => tool.name));
    let msgs = messages;
    let model = "";
    let usage: Usage = {
      promptTokens: 0,
      completionTokens: 0,
      cacheHitTokens: 0,
      cacheMissTokens: 0,
    };

    for (let round = 0; round < 4; round++) {
      const res = await this.provider.chat({
        messages: msgs,
        maxTokens: this.maxTokens,
        ...(this.temperature === undefined ? {} : { temperature: this.temperature }),
        meta: { prefixHash },
        ...(tools.length > 0 ? { tools } : {}),
      });
      usage = addUsage(usage, res.usage);
      model = res.model;

      if (!res.toolCalls || res.toolCalls.length === 0) {
        return { text: res.text, usage, model };
      }

      // Every announced call must get a matching tool result, or the next
      // request is rejected for orphaned tool_call ids.
      const calls = res.toolCalls.slice(0, 4);
      msgs = [
        ...msgs,
        {
          role: "assistant",
          content: res.text,
          toolCalls: calls,
          ...(res.reasoning ? { reasoning: res.reasoning } : {}),
        },
      ];
      for (const call of calls) {
        const output = this.runTool(call);
        msgs = [...msgs, { role: "tool", toolCallId: call.id, content: output }];
      }
    }

    const final = await this.provider.chat({
      messages: msgs,
      maxTokens: this.maxTokens,
      ...(this.temperature === undefined ? {} : { temperature: this.temperature }),
      meta: { prefixHash },
    });
    return { text: final.text, usage: addUsage(usage, final.usage), model: final.model || model };
  }

  private runTool(call: ToolCall): string {
    let args: Record<string, unknown> = {};
    try {
      const parsed = JSON.parse(call.arguments || "{}") as unknown;
      if (parsed && typeof parsed === "object") args = parsed as Record<string, unknown>;
    } catch {
      args = {};
    }

    if (!this.offered.has(call.name)) return `不認識的工具：${call.name}`;

    const extra = this.extraTools.find((tool) => tool.spec.name === call.name);
    if (extra) {
      return extra.run(args, { turn: this.turnSerial, where: this.activeWhere ?? this.where });
    }

    if (call.name === "share") {
      if (!this.shareDir) return "現在沒有東西可以分享。";
      const items = loadShare(this.shareDir);
      if (items.length === 0) return "分享夾是空的。";

      const query = String(args["query"] ?? "");
      const item = pickShare(items, query, seedOf(`${this.turnIndex}:${query}`), this.shareRecent);
      if (!item) return "找不到適合的。";

      this.pendingShare = item;
      this.shareRecent.push(item.name);
      while (this.shareRecent.length > 8) this.shareRecent.shift();

      const caption = String(args["caption"] ?? "").trim().slice(0, 80);
      const tags = item.tags.length > 0 ? `；${item.tags.join("/")}` : "";
      return (
        `挑好了：${item.name}（${item.note}${tags}）` +
        (caption === "" ? "" : `，配文：${caption}`) +
        "。這張會跟你的回覆一起送出，不用再描述畫面。"
      );
    }

    if (call.name === "relationship") {
      if (!this.memory) return "沒有記憶可用。";
      const name = String(args["person"] ?? "").trim();
      const roster = [...this.roster.values()];
      const target =
        roster.find((person) => person.name === name) ??
        roster.find((person) => person.name.includes(name) || name.includes(person.name));
      if (!target) return `這裡沒有叫「${name}」的人。`;

      const patch: PersonPatch = {};
      if (typeof args["closeness"] === "number") patch.closeness = args["closeness"];
      if (typeof args["warmth"] === "number") patch.warmth = args["warmth"];
      if (typeof args["impression"] === "string") patch.impression = args["impression"];
      if (typeof args["note"] === "string") patch.note = args["note"];
      if (typeof args["habit"] === "string") patch.habit = args["habit"];
      if (typeof args["pending"] === "string") patch.pending = args["pending"];
      if (typeof args["joke"] === "string") patch.joke = args["joke"];
      if (typeof args["resolve"] === "string") patch.resolve = args["resolve"];
      if (typeof args["grudge_minutes"] === "number") patch.grudgeMinutes = args["grudge_minutes"];
      if (typeof args["reason"] === "string") patch.reason = args["reason"];

      const updated = this.memory.setPerson(target.id, patch, this.now());
      if (!updated) return "更新失敗。";

      const bits = [`對 ${updated.name}：${closenessBand(updated.closeness)}`];
      if (updated.impression) bits.push(updated.impression);
      if (updated.habits.length > 0) bits.push(`記得 ${updated.habits.length} 個習慣`);
      if (updated.pending.length > 0) bits.push(`${updated.pending.length} 件還沒結`);
      if (updated.jokes.length > 0) bits.push(`${updated.jokes.length} 個內部梗`);
      return bits.join("；");
    }

    if (call.name === "self") {
      const patch: SelfPatch = {};
      const moodPatch: Partial<MoodState> = {};
      const num = (key: string): number | undefined =>
        typeof args[key] === "number" ? (args[key] as number) : undefined;

      const hunger = num("hunger");
      if (hunger !== undefined) moodPatch.hunger = Math.min(100, Math.max(0, hunger));
      const energy = num("energy");
      if (energy !== undefined) moodPatch.energy = Math.min(100, Math.max(0, energy));
      const boredom = num("boredom");
      if (boredom !== undefined) patch.boredom = boredom;

      const mood = args["mood"];
      if (typeof mood === "string" && (MOODS as readonly string[]).includes(mood)) {
        patch.mood = mood as MoodState["mood"];
      }
      const intensity = num("intensity");
      if (intensity === 1 || intensity === 2 || intensity === 3) patch.intensity = intensity;

      if (typeof args["slacking"] === "boolean") patch.slacking = args["slacking"];
      if (typeof args["task"] === "string") patch.task = args["task"];
      if (typeof args["pending"] === "string") patch.pending = args["pending"];
      if (args["clear_pending"] === true) patch.pending = null;
      if (args["clear_plan"] === true) patch.clearPlan = true;

      const planMinutes = num("plan_minutes");
      if (planMinutes !== undefined) patch.planMinutes = planMinutes;
      if (typeof args["plan_topic"] === "string") patch.planTopic = args["plan_topic"];
      if (typeof args["plan_why"] === "string") patch.planWhy = args["plan_why"];

      if (Object.keys(moodPatch).length > 0) {
        this.pendingMood = { ...(this.pendingMood ?? {}), ...moodPatch };
      }

      const applied = applySelf(this.selfState, patch, this.now());
      this.selfState = applied.state;
      this.selfStore?.save(applied.state);
      if (applied.log.length > 0) this.onSelfChange?.(applied.state, applied.log);

      return applied.log.length === 0 ? "沒有改變。" : `好：${applied.log.join("；")}`;
    }

    if (call.name === "persona_note") {
      if (!this.notesDir) return "現在不能寫筆記。";
      const line = appendNote(this.notesDir, String(args["note"] ?? ""));
      return line === "" ? "沒有寫入。" : `記下了：${line}`;
    }

    if (!this.memory) return "沒有記憶可用。";

    if (call.name === "memory_search") {
      const query = String(args["query"] ?? "");
      const limit = Math.min(8, Math.max(1, Number(args["limit"] ?? 6) || 6));
      const hits = this.memory.search(query, limit);
      if (hits.length === 0) return "沒有找到相關內容。";
      return hits.map((h) => `${h.path}#L${h.line} (${h.score.toFixed(2)}) ${h.why}`).join("\n");
    }

    if (call.name === "memory_open") {
      const path = String(args["path"] ?? "");
      const from = Math.max(1, Number(args["from_line"] ?? 1) || 1);
      const lines = Math.min(80, Math.max(1, Number(args["lines"] ?? 40) || 40));
      const text = this.memory.open(path, from, lines);
      return text === "" ? "讀不到這個路徑。" : text.slice(0, 2000);
    }

    if (call.name === "memory_diary") {
      const raw = args["date"];
      const date =
        typeof raw === "string" && /^\d{4}-\d{2}-\d{2}$/.test(raw)
          ? new Date(`${raw}T12:00:00`)
          : this.now();
      const text = this.memory.diary(date);
      return text === null ? "那天還沒有日記。" : text.slice(0, 2000);
    }

    if (call.name === "memory_timeline") {
      const raw = args["date"];
      const date =
        typeof raw === "string" && /^\d{4}-\d{2}-\d{2}$/.test(raw)
          ? new Date(`${raw}T12:00:00`)
          : this.now();
      const lines = this.memory.timeline(date);
      if (lines.length === 0) return `${formatNow(date).slice(0, 10)} 那天沒有紀錄。`;
      return `${formatNow(date).slice(0, 10)}（${lines.length} 筆）\n${lines.slice(-40).join("\n").slice(0, 2000)}`;
    }

    return `不認識的工具：${call.name}`;
  }

  private closeness(): "stranger" | "friend" | "close" {
    if (this.turnIndex < 4) return "stranger";
    if (this.turnIndex < 24) return "friend";
    return "close";
  }

  /**
   * Log a message she merely overheard, and remember the speaker. Called for
   * every message in scope, including ones she decides not to answer — the
   * journal is a record of the room, not of her replies.
   */
  logHeard(input: HeardInput): void {
    const now = input.at ?? this.now();

    if (input.speaker) {
      this.roster.set(input.speaker.id, {
        id: input.speaker.id,
        name: input.speaker.name,
        lastAt: now.getTime(),
      });
    }

    if (!this.memory) return;
    try {
      this.memory.log({
        kind: "heard",
        text: input.text,
        at: now,
        where: input.where ?? this.where,
        who: input.speaker?.name,
        whoId: input.speaker?.id,
      });
    } catch {
      // logging must never cost a reply
    }
  }

  async turn(userText: string, opts: TurnOptions = {}): Promise<TurnResult> {
    const now = this.now();
    this.pendingShare = null;
    const where = opts.where ?? this.where;
    this.activeWhere = where;
    this.toolLevel = opts.tools ?? "full";
    this.turnSerial++;
    const memoryOpen = this.toolLevel === "full";
    this.selfState = tickSelf(
      this.selfStore?.load() ?? this.selfState,
      now,
      this.lastAt?.getTime() ?? null,
    );

    if (this.personaLoader) {
      const blocks = this.personaLoader();
      const nextHash = hashBlocks(blocks);
      if (nextHash !== this.personaHash) {
        if (this.personaHash !== "") console.log("[persona] reloaded");
        this.persona = blocks;
        this.personaHash = nextHash;
        // A reload is a deliberate new prefix. Without this, the guard below
        // sees the change as a leak and refuses every turn until restart.
        this.lastPrefix = "";
      }
    }

    // 2 TICK — physics first, so the tip reflects the current hour.
    let mood = advanceTurn(this.moodStore?.load() ?? this.mood);
    mood = tick(mood, now);
    mood = onMessage(mood);

    const gapHours = this.lastAt
      ? (now.getTime() - this.lastAt.getTime()) / 3_600_000
      : 0;
    const ctx: MoodContext = {
      closeness: opts.owner ? "close" : this.closeness(),
      gapHours,
    };

    if (opts.speaker) {
      this.roster.set(opts.speaker.id, {
        id: opts.speaker.id,
        name: opts.speaker.name,
        lastAt: now.getTime(),
      });
    }

    // A bracketed speaker tag keeps names from being read as sentence text,
    // and gives the roster something to point at.
    const lines = (opts.prior ?? []).map((p) => `[${p.name}] ${p.text}`);
    if (opts.group && opts.speaker) lines.push(`[${opts.speaker.name}] ${userText}`);
    const body = lines.length > 0 ? lines.join("\n") : userText;
    const userContent = opts.context ? `${opts.context}\n\n${body}` : body;

    // 3 TIP — the only volatile bytes, and they live at the very end.
    // Memory pointers come first, mood last: the newest frame wins.
    const moodTip = moodBlock(mood, now, ctx);
    // Nothing from the vault reaches a turn that has no memory tools.
    const recalled = memoryOpen ? (this.memory?.recall(userText, now) ?? null) : null;
    const people =
      opts.group && opts.speaker
        ? renderPeople([...this.roster.values()], opts.speaker.id)
        : "";
    const ownerNote = opts.owner
      ? "（這是你的主人。可以主動一點、話多一點，不用每句都等他問；但不要變成在討好。）"
      : "";
    const clock = `（現在：${formatNow(now)}。這是你的時鐘，回答時間問題時以它為準。）`;
    const selfNotes = this.notesDir && memoryOpen ? notesBlock(this.notesDir) : null;
    const profile =
      opts.speaker && memoryOpen ? (this.memory?.person(opts.speaker.id) ?? null) : null;
    const brief = profile ? renderPersonBrief(profile, now) : "";
    const relation = profile
      ? [
          `（你對 ${profile.name}：${closenessBand(profile.closeness)}${profile.impression ? `；${profile.impression}` : ""}）`,
          brief,
        ]
          .filter(Boolean)
          .join("\n")
      : "";

    const selfLine = renderSelf(this.selfState, now);
    const noteText = opts.note ? `（${opts.note}）` : "";
    const tipText = [
      recalled?.content,
      people,
      relation,
      selfNotes?.content,
      ownerNote,
      selfLine,
      noteText,
      moodTip.content,
      clock,
    ]
      .filter(Boolean)
      .join("\n\n");
    const gapText =
      opts.gapHours && opts.gapHours >= 1
        ? `你已經大約 ${Math.round(opts.gapHours)} 小時沒跟他說話了。`
        : "";
    const tip: Block = opts.spontaneous
      ? {
          id: "tip",
          volatility: 4,
          content:
            `${tipText}\n\n` +
            `（沒有人問你。像突然想到一樣，對主人說一句話就好。` +
            `${gapText}` +
            `${opts.planTopic ? `你之前自己說想聊「${opts.planTopic}」。` : ""}` +
            `${opts.reason ? `你想現在開口，因為：${opts.reason}。` : ""}` +
            `${opts.pending ? `有句話你一直沒說出口：「${opts.pending}」，現在可以說了。` : ""}` +
            `${opts.deferred && opts.deferred > 0 ? "這件事你本來更早就想講，忍住了。" : ""}` +
            `不要描述環境或旁人，不要旁白，不要問「在嗎」。）`,
        }
      : { id: "tip", volatility: 4, content: tipText };

    // 4 ASSEMBLE
    const hot: Msg[] = opts.spontaneous
      ? [...this.history.slice(-this.hotTurns * 2)]
      : [...this.history.slice(-this.hotTurns * 2), { role: "user", content: userContent }];
    const prompt = assemble(
      {
        static: this.persona,
        stable: this.stable,
        chain: this.chain,
        hot,
        tip,
      },
      this.budget ? { budget: this.budget } : {},
    );

    if (this.strictPrefix && this.lastPrefix !== "" && prompt.prefixHash !== this.lastPrefix) {
      throw new Error(
        `prefix changed mid-session: ${this.lastPrefix} -> ${prompt.prefixHash}. ` +
          `Volatile state leaked into the cached prefix.`,
      );
    }
    this.lastPrefix = prompt.prefixHash;

    // 5 SEND — with tools, so she can actually read the vault
    const res = await this.converse(prompt.messages, prompt.prefixHash);

    if (this.pendingMood) {
      mood = { ...mood, ...this.pendingMood };
      this.pendingMood = null;
    }

    // 6 PARSE
    const { text, expression } = parseExpression(res.text);

    // 7 EXPRESS
    let nudgeLog: string | undefined;
    if (expression.mood) {
      const r = nudge(
        mood,
        { mood: expression.mood, intensity: expression.intensity, why: expression.why },
        now,
        { nudgedThisSession: this.nudged },
      );
      if (r.accepted) {
        mood = r.state;
        this.nudged = true;
        nudgeLog = r.log;
        if (r.log) this.onMoodChange?.(mood, r.log);
      }
    }

    const seed = seedOf(`${this.turnIndex}:${userText}`);
    const kaomoji = expression.feel ? pickKaomoji(expression.feel, seed) : null;
    const tone = expression.tone ?? toneForMood(mood.mood);
    const voice = shouldVoice(
      text,
      seed,
      this.voice,
      Boolean(expression.feel) || Boolean(expression.tone),
    );
    const segments = splitMessage(text, expression.segments ?? 1);
    const whisper =
      expression.whisper &&
      shouldWhisper(seed, Number(process.env.FISHD_WHISPER_P ?? "0.3"))
        ? expression.whisper
        : null;

    // 8 PERSIST — store what she actually said, kaomoji included, so she
    // does not reuse the same face next turn.
    const display = kaomoji ? `${text} ${kaomoji}` : text;
    if (!opts.spontaneous) {
      // Store exactly what was sent, tip included. Otherwise the previous
      // prompt is not a prefix of this one and the cache dies at that message.
      const sent = prompt.messages[prompt.messages.length - 1];
      this.history.push({
        role: "user",
        content: sent && sent.role === "user" ? sent.content : userContent,
      });
    }
    this.history.push({ role: "assistant", content: display });

    this.mood = mood;
    this.moodStore?.save(mood);
    this.turnIndex++;
    this.lastAt = now;
    this.cache.record(res.usage);
    this.selfStore?.save(this.selfState);
    void this.maybeCompact();

    if (this.memory) {
      try {
        if (!opts.heardLogged && !opts.spontaneous) {
          this.memory.log({
            kind: "heard",
            text: userText,
            at: now,
            where,
            who: opts.speaker?.name,
            whoId: opts.speaker?.id,
          });
        }
        this.memory.log({
          kind: "said",
          text: display,
          at: now,
          where,
          mood: mood.mood,
          spontaneous: opts.spontaneous,
        });
      } catch {
        // A failed journal write must never cost the user a reply.
      }
    }

    return {
      segments,
      kaomoji,
      tone,
      voice,
      react: expression.react ?? null,
      whisper,
      share: this.pendingShare,
      mood,
      usage: res.usage,
      prompt,
      footer: this.cache.formatLast(res.usage),
      memory: recalled?.content ?? null,
      nudgeLog,
      model: res.model,
    };
  }
}
