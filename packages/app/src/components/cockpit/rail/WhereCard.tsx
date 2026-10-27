// Where this pane runs: the agent, its model and version, the folder, the git branch and the account.
import type { ReactNode } from 'react';
import { RailCard } from './RailCard';
import { shortFolder } from './railFormat';

export interface WhereCardProps {
  agentName: string;
  model: string | null;
  cliVersion: string | null;
  folder: string;
  /** The git branch of the folder; null when it is not a git folder (or not known yet). */
  branch: string | null;
  accountLabel?: string | null;
}

function Row({ label, children, title }: { label: string; children: ReactNode; title?: string }) {
  return (
    <div className="ck-rail-row">
      <dt>{label}</dt>
      <dd title={title}>{children}</dd>
    </div>
  );
}

export function WhereCard({ agentName, model, cliVersion, folder, branch, accountLabel }: WhereCardProps) {
  const agent = [agentName, cliVersion ? `v${cliVersion.replace(/^v/i, '')}` : null].filter(Boolean).join(' ');
  return (
    <RailCard title="Where" testId="rail-where">
      <dl className="ck-rail-facts">
        <Row label="Agent">{agent}</Row>
        {model && (
          <Row label="Model" title={model}>
            <span className="ck-mono">{model}</span>
          </Row>
        )}
        <Row label="Folder" title={folder}>
          {shortFolder(folder)}
        </Row>
        {branch && (
          <Row label="Branch" title={branch}>
            <span className="ck-mono">{branch}</span>
          </Row>
        )}
        {accountLabel && <Row label="Account">{accountLabel}</Row>}
      </dl>
    </RailCard>
  );
}
