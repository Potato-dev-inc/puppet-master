// DeepSeek key (Potato Bot's brain): presence only. The value is never read or shown.
import type { CheckRow } from './types';

export function checkDeepseekKey(saved: boolean): CheckRow[] {
  return [
    saved
      ? { id: 'deepseek-key', status: 'ok', sentence: 'DeepSeek key is saved', fix: null }
      : { id: 'deepseek-key', status: 'warn', sentence: 'No DeepSeek key yet. Fish needs one to answer.', fix: { kind: 'addKey', label: 'Add key' } },
  ];
}
