import type { ReactNode } from 'react';

export type SettingsTabId =
  | 'general'
  | 'appearance'
  | 'session'
  | 'orchestrator'
  | 'api'
  | 'mcp'
  | 'mobile'
  | 'security'
  | 'storage'
  | 'notifications'
  | 'backup'
  | 'marketplace'
  | 'rules'
  | 'costs'
  | 'team'
  | 'automation'
  | 'observability'
  | 'plugins'
  | 'developer'
  | 'advanced';

export interface SettingsTabDef {
  id: SettingsTabId;
  label: string;
  /** Entire tab is mock / no backend yet. */
  planned: boolean;
}

export const SETTINGS_TABS: SettingsTabDef[] = [
  { id: 'general', label: 'General', planned: false },
  { id: 'appearance', label: 'Appearance', planned: false },
  { id: 'session', label: 'Sessions', planned: false },
  { id: 'orchestrator', label: 'Orchestrator', planned: false },
  { id: 'api', label: 'API keys', planned: false },
  { id: 'mcp', label: 'MCP & bridge', planned: false },
  { id: 'mobile', label: 'Mobile PWA', planned: false },
  { id: 'security', label: 'Security', planned: true },
  { id: 'storage', label: 'Storage', planned: false },
  { id: 'notifications', label: 'Notifications', planned: true },
  { id: 'backup', label: 'Backup & sync', planned: true },
  { id: 'marketplace', label: 'Agent marketplace', planned: true },
  { id: 'rules', label: 'Rules & guardrails', planned: true },
  { id: 'costs', label: 'Cost controls', planned: true },
  { id: 'team', label: 'Team workspace', planned: true },
  { id: 'automation', label: 'Automation', planned: true },
  { id: 'observability', label: 'Observability', planned: true },
  { id: 'plugins', label: 'Plugins', planned: true },
  { id: 'developer', label: 'Developer', planned: false },
  { id: 'advanced', label: 'Advanced', planned: false },
];

export function PlannedBadge({ compact }: { compact?: boolean }) {
  return (
    <span
      className={[
        'inline-flex shrink-0 items-center gap-1 rounded-full border border-red-500/40 bg-red-500/10 font-medium text-red-400',
        compact ? 'px-1.5 py-0.5 text-[9px]' : 'px-2 py-0.5 text-[10px]',
      ].join(' ')}
      title="Not wired to a backend yet"
    >
      <span className="h-1.5 w-1.5 rounded-full bg-red-500" aria-hidden />
      {compact ? 'Planned' : 'Not implemented'}
    </span>
  );
}

export function LiveBadge() {
  return (
    <span className="inline-flex shrink-0 items-center gap-1 rounded-full border border-emerald-500/30 bg-emerald-500/10 px-2 py-0.5 text-[10px] font-medium text-emerald-400">
      <span className="h-1.5 w-1.5 rounded-full bg-emerald-500" aria-hidden />
      Live
    </span>
  );
}

export function SettingsSection({
  title,
  description,
  children,
}: {
  title: string;
  description: string;
  children: ReactNode;
}) {
  return (
    <div>
      <div className="mb-5">
        <h2 className="text-lg font-semibold tracking-tight">{title}</h2>
        <p className="mt-1 text-sm leading-6 text-pm-muted">{description}</p>
      </div>
      <div className="space-y-4">{children}</div>
    </div>
  );
}

export function SettingBlock({
  label,
  description,
  implemented,
  children,
}: {
  label: string;
  description?: string;
  implemented: boolean;
  children: ReactNode;
}) {
  return (
    <div
      className={[
        'rounded-xl border border-pm-border bg-pm-panel p-4',
        implemented ? '' : 'border-red-500/20',
      ].join(' ')}
    >
      <div className="mb-3 flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <h3 className="text-sm font-semibold">{label}</h3>
            {!implemented && <PlannedBadge />}
          </div>
          {description && <p className="mt-1 text-sm leading-6 text-pm-muted">{description}</p>}
        </div>
      </div>
      <div className={implemented ? '' : 'pointer-events-none opacity-60'}>{children}</div>
    </div>
  );
}

export function FieldLabel({ children }: { children: ReactNode }) {
  return <label className="mb-1 block text-xs font-medium text-pm-muted">{children}</label>;
}

export function FieldInput(props: React.InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      {...props}
      className={[
        'w-full rounded-lg border border-pm-border bg-pm-bg px-3 py-2 text-sm outline-none focus:border-pm-accent/50',
        props.className ?? '',
      ].join(' ')}
    />
  );
}

export function FieldSelect(props: React.SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <select
      {...props}
      className={[
        'w-full rounded-lg border border-pm-border bg-pm-bg px-3 py-2 text-sm outline-none focus:border-pm-accent/50',
        props.className ?? '',
      ].join(' ')}
    />
  );
}

export function SettingToggle({
  label,
  description,
  checked,
  onChange,
  implemented,
}: {
  label: string;
  description: string;
  checked: boolean;
  onChange: (value: boolean) => void;
  implemented: boolean;
}) {
  return (
    <SettingBlock label={label} description={description} implemented={implemented}>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        disabled={!implemented}
        onClick={() => implemented && onChange(!checked)}
        className={[
          'relative h-7 w-12 shrink-0 rounded-full transition',
          checked ? 'bg-pm-accent' : 'bg-pm-border',
          implemented ? '' : 'cursor-not-allowed',
        ].join(' ')}
      >
        <span
          className={[
            'absolute top-1 h-5 w-5 rounded-full bg-pm-bg shadow-sm transition',
            checked ? 'left-6' : 'left-1',
          ].join(' ')}
        />
      </button>
    </SettingBlock>
  );
}

export function InfoCard({
  title,
  description,
  implemented = true,
}: {
  title: string;
  description: string;
  implemented?: boolean;
}) {
  return (
    <div className="rounded-xl border border-pm-border bg-pm-raised p-4">
      <div className="flex items-start gap-3">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <h3 className="text-sm font-semibold">{title}</h3>
            {implemented ? null : <PlannedBadge compact />}
          </div>
          <p className="mt-1 text-sm leading-6 text-pm-muted">{description}</p>
        </div>
      </div>
    </div>
  );
}

export function CodeBlock({ title, code }: { title: string; code: string }) {
  return (
    <div className="overflow-hidden rounded-xl border border-pm-border bg-pm-panel">
      <div className="flex items-center justify-between border-b border-pm-border px-4 py-2">
        <h3 className="text-sm font-semibold">{title}</h3>
        <button
          type="button"
          onClick={() => void navigator.clipboard.writeText(code)}
          className="rounded-lg border border-pm-border px-2 py-1 text-xs text-pm-muted hover:bg-pm-border/40"
        >
          Copy
        </button>
      </div>
      <pre className="overflow-auto p-4 font-mono text-xs leading-6 text-pm-muted">{code}</pre>
    </div>
  );
}

export function MockListCard({
  title,
  description,
  items,
}: {
  title: string;
  description: string;
  items: Array<{ title: string; meta: string; status?: string }>;
}) {
  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-2">
        <h3 className="text-sm font-semibold">{title}</h3>
        <PlannedBadge />
      </div>
      <p className="text-sm text-pm-muted">{description}</p>
      <div className="space-y-2 opacity-70">
        {items.map((item) => (
          <div
            key={item.title}
            className="flex items-center justify-between gap-3 rounded-xl border border-pm-border border-red-500/15 bg-pm-panel p-4"
          >
            <div className="min-w-0">
              <div className="text-sm font-semibold">{item.title}</div>
              <div className="mt-1 text-xs text-pm-muted">{item.meta}</div>
            </div>
            {item.status && (
              <span className="shrink-0 rounded-full bg-pm-border/40 px-2 py-0.5 text-xs text-pm-muted">
                {item.status}
              </span>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}

export function StorageBarMock({ label, value, percent }: { label: string; value: string; percent: string }) {
  return (
    <div className="rounded-xl border border-pm-border border-red-500/15 bg-pm-panel p-4 opacity-70">
      <div className="flex items-center justify-between">
        <div className="text-sm font-semibold">{label}</div>
        <div className="text-sm text-pm-muted">{value}</div>
      </div>
      <div className="mt-3 h-2 overflow-hidden rounded-full bg-pm-border/40">
        <div className={`h-full rounded-full bg-pm-muted/50 ${percent}`} />
      </div>
    </div>
  );
}
