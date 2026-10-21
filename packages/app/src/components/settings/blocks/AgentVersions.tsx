// Settings > Agents > Agent versions: is each agent a version Puppet Master has been tested with, did
// its start-up screens still read fine the last time they were checked, and which screens did
// Puppet Master fail to recognise. Backed by the `canary_*` commands (src-tauri/src/canary).
//
// Copy follows UX-PHASE1 §0: "Untested version" is the only chip, no code names or raw ids.
import { useCallback, useState } from 'react';
import { changeLine, lastCheckLine, tauriCanaryApi, useCanary, type CanaryApi } from '../../../lib/canary';
import { Btn, Note, SettingBlock, SettingRow, SettingToggle } from '../settings-ui';
import '../../../styles/canary.css';

export function AgentVersions({ api = tauriCanaryApi, now = () => Date.now() }: { api?: CanaryApi; now?: () => number }) {
  const { status, refresh } = useCanary(api);
  const [running, setRunning] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);
  const installed = status.agents.filter((a) => a.installed);

  const check = useCallback(
    async (agent: string) => {
      setError(null);
      setRunning((cur) => new Set(cur).add(agent));
      try {
        await api.check(agent);
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      } finally {
        setRunning((cur) => {
          const next = new Set(cur);
          next.delete(agent);
          return next;
        });
        refresh();
      }
    },
    [api, refresh],
  );

  return (
    <>
      <SettingBlock
        label="Agent versions"
        description="Agents update often, and an update can change what Puppet Master sees on screen. Checking starts the agent in an empty folder, waits for its input box, then closes it. Nothing is sent to it, so it costs no tokens."
      >
        <ul className="st-ver-list" aria-label="Agent versions" data-testid="agent-versions">
          {installed.map((agent) => {
            const busy = agent.checking || running.has(agent.agent);
            const last = lastCheckLine(agent, now());
            const change = changeLine(agent, now());
            return (
              <li key={agent.agent} className="st-ver-row" data-testid={`agent-version-${agent.agent}`}>
                <div className="st-ver-main">
                  <div className="st-ver-line">
                    <span className="st-ver-name">{agent.label}</span>
                    {agent.version && <span className="st-ver-num">{agent.version}</span>}
                    {agent.chip && (
                      <span className="st-ver-chip" data-testid={`agent-version-chip-${agent.agent}`}>
                        {agent.chip}
                      </span>
                    )}
                  </div>
                  <p className="st-ver-plain">{agent.plain}</p>
                  {change && (
                    <p className="st-ver-change" data-verdict={agent.change?.verdict} data-testid={`agent-version-change-${agent.agent}`}>
                      {change}
                    </p>
                  )}
                  {busy ? (
                    <p className="st-ver-last">Checking. This can take up to a minute.</p>
                  ) : (
                    last && <p className="st-ver-last">{last}</p>
                  )}
                </div>
                <div className="st-ver-actions">
                  <Btn small disabled={busy} onClick={() => void check(agent.agent)}>
                    {busy ? 'Checking' : 'Check this agent'}
                  </Btn>
                </div>
              </li>
            );
          })}
          {installed.length === 0 && <li className="st-ver-plain">No agents are installed on this computer yet.</li>}
        </ul>
        {error && <Note role="alert">{error}</Note>}
      </SettingBlock>
      <SettingToggle
        label="Check agents after they update"
        description="When an agent's version changes, check it the same way. Off: only when you press Check this agent."
        checked={status.auto_check}
        onChange={(on) => void api.setAuto(on).then(refresh)}
      />
      <SettingRow
        label="Screens I didn't recognise"
        description={
          status.captures === 0
            ? 'When an agent stays on a screen Puppet Master cannot read, the screen is saved here with secrets removed. Nothing has been saved yet.'
            : `${status.captures} ${status.captures === 1 ? 'screen' : 'screens'} saved on this computer, with secrets removed. Nothing is sent anywhere.`
        }
      >
        <Btn small onClick={() => void api.openCaptures().catch((e) => setError(e instanceof Error ? e.message : String(e)))}>
          Open folder
        </Btn>
      </SettingRow>
    </>
  );
}
