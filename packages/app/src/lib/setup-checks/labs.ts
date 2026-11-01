// Labs: one summary row, which Labs features are on. Nothing to fix here: the switches live in General > Labs.
import { DEV_LABS_KEYS } from '../dev-labs';
import type { CheckRow, SetupCheckContext } from './types';

const LABEL: Record<string, string> = {
  missions_enabled: 'Missions',
  night_shift_enabled: 'Night shift',
  potato_bot_enabled: 'Potato Bot',
  potato_sandbox_enabled: "Fish's computer",
  acp_chat_enabled: 'Chat',
  desk_enabled: 'Desk',
  model_bridge_enabled: 'Model Bridge',
};

export function checkLabs(ctx: SetupCheckContext): CheckRow[] {
  const on = DEV_LABS_KEYS.filter((k) => ctx.labs[k] === true).map((k) => LABEL[k] ?? k);
  const sentence = on.length === 0 ? 'No Labs features are on' : `Labs on: ${on.join(', ')}`;
  return [{ id: 'labs', status: 'ok', sentence, fix: null }];
}
