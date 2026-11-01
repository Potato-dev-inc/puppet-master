// Starter bots (F-2): six ready-made bot recipes for the "New bot" picker. The recipes are data files in
// src/data/potato-starters/ (JSON, one per card, checked here with zod). They hold a name, a face, one sentence
// of what the bot does, instructions, the three tool switches and one routine. They never hold secrets or memory.
// `makeStarterBot` uses the existing calls only: companion create, bot add, instructions save, tools set, routine create.
import { z } from 'zod';
import type { AvatarId, CompanionPersona, CompanionTransport, CompanionView } from './companion';
import type { PotatoTransport } from './potato';
import { INSTRUCTIONS_MAX, type BotSettingsTransport, type ToolSwitches } from './potato-botsettings';
import type { RoutineInput, RoutinesTransport } from './potato-routines';

const AVATAR_IDS = ['mochi', 'bit', 'nib', 'tofu', 'lumen', 'hoot', 'sprout', 'kit'] as const;
const TIME = /^([01]\d|2[0-3]):[0-5]\d$/;

const scheduleSchema = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('hourly') }).strict(),
  z.object({ kind: z.literal('daily'), time: z.string().regex(TIME) }).strict(),
  z.object({ kind: z.literal('weekdays'), time: z.string().regex(TIME) }).strict(),
  z.object({ kind: z.literal('everyMinutes'), minutes: z.number().int().min(15).max(1440) }).strict(),
]);

export const starterSchema = z
  .object({
    id: z.string().regex(/^[a-z-]+$/),
    name: z.string().min(1).max(40),
    avatar: z.enum(AVATAR_IDS),
    doing: z.string().min(1).max(200),
    instructions: z.string().min(1).max(INSTRUCTIONS_MAX).optional(),
    tools: z
      .object({ helpers: z.boolean(), computer: z.boolean(), web: z.boolean() })
      .strict()
      .optional(),
    routine: z
      .object({
        title: z.string().min(1).max(60),
        prompt: z.string().min(1).max(1000),
        schedule: scheduleSchema,
      })
      .strict()
      .optional(),
    needsComputer: z.boolean(),
  })
  .strict();

export type StarterTemplate = z.infer<typeof starterSchema>;

/** The raw JSON of every template file, in file-name order. The tests read these too. */
export function loadStarterFiles(): { path: string; data: unknown }[] {
  const files = import.meta.glob('../data/potato-starters/*.json', { eager: true, import: 'default' }) as Record<string, unknown>;
  return Object.keys(files)
    .sort()
    .map((path) => ({ path, data: files[path] }));
}

/** The validated templates. A file that breaks the schema throws here, so the app never shows a bad card. */
export const STARTERS: readonly StarterTemplate[] = loadStarterFiles().map((file) => starterSchema.parse(file.data));

export const STARTER_COPY = {
  title: 'New bot',
  intro: 'Pick a starting point. You can change everything later.',
  needsComputer: "Needs Fish's computer",
  setByHand: 'Set up by hand',
  blankName: 'Blank',
};

const DEFAULT_TOOLS: ToolSwitches = { helpers: true, computer: true, web: true };
const NAP = { h: 4, m: 0 };
const PERSONA: CompanionPersona = 'plain';

export interface StarterTransports {
  companions: Pick<CompanionTransport, 'agents' | 'create'>;
  potato: Pick<PotatoTransport, 'addBot'>;
  settings: Pick<BotSettingsTransport, 'saveInstructions' | 'setTools'>;
  routines: Pick<RoutinesTransport, 'create'>;
}

/** The routine a template asks for, in the shape the routines call takes. */
export function starterRoutine(starter: StarterTemplate): RoutineInput | null {
  if (!starter.routine) return null;
  return { title: starter.routine.title, prompt: starter.routine.prompt, schedule: starter.routine.schedule };
}

/**
 * Makes the bot a template describes and returns its id. Blank makes a plain bot only. The first ready agent runs it
 * (the same default the setup sheet picks), and the folder is the default one.
 */
export async function makeStarterBot(starter: StarterTemplate, transports: StarterTransports): Promise<string> {
  const agents = await transports.companions.agents();
  const agentType = agents.find((a) => a.ok)?.id;
  if (!agentType) throw new Error('No agent is ready yet. Set one up first.');
  const view: CompanionView = await transports.companions.create({
    name: starter.name,
    avatar: starter.avatar as AvatarId,
    persona: PERSONA,
    agentType,
    folder: '',
    autoStart: true,
    memoryReview: true,
    napAt: NAP,
    checkins: [],
  });
  const botId = view.id;
  await transports.potato.addBot(botId);
  if (starter.instructions) await transports.settings.saveInstructions(botId, starter.instructions);
  if (starter.tools) await transports.settings.setTools(botId, { ...DEFAULT_TOOLS, ...starter.tools });
  const routine = starterRoutine(starter);
  if (routine) await transports.routines.create(botId, routine);
  return botId;
}
