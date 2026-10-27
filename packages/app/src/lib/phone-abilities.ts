// What THIS phone may do, read from `GET /pair/me` (bridge/routes/pairing.rs). The PC enforces every
// ability; this only keeps the phone from offering what the PC would refuse (device-caps).
import type { DeviceCap } from '@puppet-master/shared';

/** The body of `GET /pair/me`. */
export interface PairMe {
  local: boolean;
  device_id?: string;
  caps: string[];
}

export interface PhoneAbilities {
  /** Read pane text: the chat of an agent pane needs it. */
  canWatch: boolean;
  canType: boolean;
  canAnswer: boolean;
  /** Start, stop or sleep an agent (companions: wake and sleep). */
  canControl: boolean;
  /** A plain line to show in place of the input box, or null when this phone can type. */
  typingNote: string | null;
}

/** Until the PC has answered, and on an old PC that has no such route, nothing is hidden. */
export const ALL_ABILITIES: PhoneAbilities = { canWatch: true, canType: true, canAnswer: true, canControl: true, typingNote: null };

export function abilitiesFrom(me: PairMe | null | undefined): PhoneAbilities {
  if (!me || !Array.isArray(me.caps)) return ALL_ABILITIES;
  const has = (cap: DeviceCap) => me.caps.includes(cap);
  const canWatch = has('watch');
  const canType = has('type');
  const canAnswer = has('answer');
  const canControl = has('control');
  if (canType) return { canWatch, canType, canAnswer, canControl, typingNote: null };
  return {
    canWatch,
    canType,
    canAnswer,
    canControl,
    typingNote: canAnswer ? 'This phone can watch and answer, not type.' : 'This phone can watch, not type.',
  };
}
