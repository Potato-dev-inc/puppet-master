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
  canType: boolean;
  canAnswer: boolean;
  /** A plain line to show in place of the input box, or null when this phone can type. */
  typingNote: string | null;
}

/** Until the PC has answered, and on an old PC that has no such route, nothing is hidden. */
export const ALL_ABILITIES: PhoneAbilities = { canType: true, canAnswer: true, typingNote: null };

export function abilitiesFrom(me: PairMe | null | undefined): PhoneAbilities {
  if (!me || !Array.isArray(me.caps)) return ALL_ABILITIES;
  const has = (cap: DeviceCap) => me.caps.includes(cap);
  const canType = has('type');
  const canAnswer = has('answer');
  if (canType) return { canType, canAnswer, typingNote: null };
  return {
    canType,
    canAnswer,
    typingNote: canAnswer ? 'This phone can watch and answer, not type.' : 'This phone can watch, not type.',
  };
}
