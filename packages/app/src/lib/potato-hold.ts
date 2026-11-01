// "Stop everything" for Potato Bot (UX-1). Backend: src-tauri potato/hold.rs and commands/potato_hold.rs.
import { invoke } from '@tauri-apps/api/core';

/** What the hold stopped. Bots and routines run again only after `potatoResumeAll`. */
export interface PotatoHold {
  held: boolean;
  sinceMs: number | null;
  botsPaused: number;
  routinesHeld: number;
  computerAsleep: boolean;
}

export const potatoHoldState = () => invoke<PotatoHold>('potato_hold_state');

/** "Start bots again": clears the hold. Agents stay stopped. */
export const potatoResumeAll = () => invoke<PotatoHold>('potato_resume_all');
