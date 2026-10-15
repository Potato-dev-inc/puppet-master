// Settings > General > Safety (the rows are placed by settings/arrange.tsx; Bridge & MCP keeps a link): what agents may do to panes and work they did not start, and a
// read-only list of recent approvals and control changes (the audit log, src-tauri/src/audit).
//
// The two switches are kept by the app itself (`agent_authority.json`), not in settings.json, so a
// paired phone cannot flip them over the bridge. UX-PHASE1 13.3 places "merge" under Orchestrator &
// Models and the list under General until Security has a section of its own; they sit together here
// so the three belong to one story. Moving a row is a one-line change in the section files.
import { useCallback, useEffect, useState } from 'react';
import {
  agentAuthorityApi,
  whoText,
  type AgentAuthority,
  type AgentAuthorityApi,
  type AuditEntry,
  type AuditVerify,
} from '../../../../lib/agent-authority-api';
import type { RowDef } from '../../model';
import { Btn, Note, SettingBlock, SettingToggle } from '../../settings-ui';

const ASK_LABEL = "Ask me before an agent controls a pane it didn't start";
const ASK_HINT =
  'An agent that did not open a pane asks you first, in the Inbox, before it types into the pane or answers its questions. Agents keep working with the helpers they started without asking.';
const MERGE_LABEL = 'Let agents merge separate copies';
const MERGE_HINT =
  'When off, an agent can show you what changed in a separate copy, but only you can merge it, from Changes. Turn on to let an agent merge by itself.';

/** "14:12" for today, "Oct 13, 14:12" for an earlier day. */
export function whenText(ts: number, now: number = Date.now()): string {
  const time = new Date(ts).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hour12: false });
  const same = new Date(ts).toDateString() === new Date(now).toDateString();
  if (same) return time;
  const day = new Date(ts).toLocaleDateString([], { month: 'short', day: 'numeric' });
  return `${day}, ${time}`;
}

function useAuthority(api: AgentAuthorityApi) {
  const [value, setValue] = useState<AgentAuthority | null>(null);
  useEffect(() => {
    void api.get().then(setValue).catch(() => undefined);
  }, [api]);
  const change = useCallback(
    async (patch: Partial<AgentAuthority>, onSaved: () => void, onError: (m: string) => void) => {
      const before = value;
      setValue((v) => (v ? { ...v, ...patch } : v));
      try {
        setValue(await api.set(patch));
        onSaved();
      } catch (e) {
        setValue(before);
        onError(`Not saved: ${e instanceof Error ? e.message : String(e)}`);
      }
    },
    [api, value],
  );
  return { value, change };
}

export function AskBeforeControlToggle({
  markSaved,
  setError,
  api = agentAuthorityApi,
}: {
  markSaved: () => void;
  setError: (message: string | null) => void;
  api?: AgentAuthorityApi;
}) {
  const { value, change } = useAuthority(api);
  return (
    <SettingToggle
      label={ASK_LABEL}
      description={ASK_HINT}
      checked={Boolean(value?.ask_before_agent_control)}
      disabled={!value}
      onChange={(on) => void change({ ask_before_agent_control: on }, markSaved, setError)}
    />
  );
}

export function AgentsMayMergeToggle({
  markSaved,
  setError,
  api = agentAuthorityApi,
}: {
  markSaved: () => void;
  setError: (message: string | null) => void;
  api?: AgentAuthorityApi;
}) {
  const { value, change } = useAuthority(api);
  return (
    <SettingToggle
      label={MERGE_LABEL}
      description={MERGE_HINT}
      checked={Boolean(value?.agents_may_merge)}
      disabled={!value}
      onChange={(on) => void change({ agents_may_merge: on }, markSaved, setError)}
    />
  );
}

/** The newest lines of the audit log, with a check that the log has not been edited. */
export function RecentApprovals({ api = agentAuthorityApi, now }: { api?: AgentAuthorityApi; now?: number }) {
  const [entries, setEntries] = useState<AuditEntry[] | null>(null);
  const [check, setCheck] = useState<AuditVerify | null>(null);
  const [failed, setFailed] = useState(false);
  const load = useCallback(async () => {
    try {
      setFailed(false);
      const [list, verified] = await Promise.all([api.auditRecent(20), api.auditVerify()]);
      setEntries(list);
      setCheck(verified);
    } catch {
      setFailed(true);
    }
  }, [api]);
  useEffect(() => {
    void load();
  }, [load]);

  return (
    <SettingBlock
      label="Recent approvals and control changes"
      description="Who allowed what, and when. Only you can read it here; nothing in it can be changed from this window."
    >
      {failed && <Note role="alert">The list could not be read.</Note>}
      {entries && entries.length === 0 && <Note>Nothing yet. Approvals, pairing and agent control show up here.</Note>}
      {entries && entries.length > 0 && (
        <ul className="st-audit" aria-label="Recent approvals and control changes">
          {entries.map((e) => (
            <li key={e.seq} className="st-audit-row" data-kind={e.kind}>
              <span className="st-audit-when">{whenText(e.ts_ms, now)}</span>
              <span className="st-audit-what">{e.summary}</span>
              <span className="st-audit-who">{whoText(e.who)}</span>
            </li>
          ))}
        </ul>
      )}
      {check && (
        <p className="st-audit-check" role={check.ok ? undefined : 'alert'}>
          {check.ok
            ? `Log checked: ${check.entries} ${check.entries === 1 ? 'entry' : 'entries'}, none changed.`
            : `This log was changed after it was written (${check.broken?.reason ?? 'a line does not match'}).`}
        </p>
      )}
      <Btn onClick={() => void load()}>Refresh</Btn>
    </SettingBlock>
  );
}

export const securityRows: RowDef[] = [
  {
    id: 'security-ask-before-control',
    label: ASK_LABEL,
    hint: ASK_HINT,
    keywords: 'agent control take over pane consent ask permission lineage security answer prompt',
    group: 'Security',
    render: (env) => <AskBeforeControlToggle markSaved={env.markSaved} setError={env.setError} />,
  },
  {
    id: 'security-agents-merge',
    label: MERGE_LABEL,
    hint: MERGE_HINT,
    keywords: 'merge separate copy worktree agent review_worktree security branch',
    group: 'Security',
    render: (env) => <AgentsMayMergeToggle markSaved={env.markSaved} setError={env.setError} />,
  },
  {
    id: 'security-recent-approvals',
    label: 'Recent approvals and control changes',
    hint: 'Who allowed what, and when.',
    keywords: 'audit log who did what approvals history security pairing shell take over',
    group: 'Security',
    render: () => <RecentApprovals />,
  },
];
