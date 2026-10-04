// Shared types for the settings tab panels: the context object every tab receives, plus the blank custom-model draft.
import type { LlmModel, Settings } from '@puppet-master/shared';
import type { UpdateCheckResult } from '../../../lib/app-update';

export const EMPTY_CUSTOM: LlmModel = {
  provider: 'openrouter',
  model_id: '',
  label: '',
};

export interface SettingsTabContext {
  settings: Settings;
  setSettings: (next: Settings) => void;
  projectPath: string | null;
  onProjectPathChange?: (path: string) => Promise<void>;
  bridgeUrl: string | null;
  draftCustom: LlmModel;
  setDraftCustom: (next: LlmModel) => void;
  onSidebarWidthChange?: (width: number) => void;
  /** Live workspace sidebar width (e.g. after drag resize). */
  currentSidebarWidth?: number;
  updateCheck?: UpdateCheckResult | null;
  updateChecking?: boolean;
  onCheckForUpdates?: () => void;
  onOpenRelease?: () => void;
}
