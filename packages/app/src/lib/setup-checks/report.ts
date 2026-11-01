// "Copy report": the rows as plain lines. Every line passes the personal-path scrubber (folder paths with a user name
// become "~") and a key scrubber (anything shaped like an API key becomes "[key removed]").
import { maskPaths } from '../../components/cockpit/what-broke-it-result-text';
import type { CheckRow } from './types';

const MARK: Record<CheckRow['status'], string> = { ok: 'OK', warn: 'CHECK', fail: 'PROBLEM' };

/** Removes folder paths that show a user name, and key-looking strings. */
export function redactReport(text: string): string {
  return maskPaths(text).replace(/\b(?:sk|pk|rk|ghp|gho|xox[abp])[-_][A-Za-z0-9_-]{6,}/g, '[key removed]');
}

export function buildReport(rows: CheckRow[], version: string, when: string): string {
  const lines = [`Puppet Master setup check (${version}, ${when})`, ''];
  for (const r of rows) lines.push(`[${MARK[r.status]}] ${r.sentence}`);
  return redactReport(lines.join('\n'));
}
