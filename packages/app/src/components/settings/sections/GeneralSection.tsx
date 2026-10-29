// Settings > General: project folder, appearance, safety nets (confirm close, loop guard, checkpoints,
// project memory, away digest), coordination storage, version and updates, export and uninstall.
// This section is the reference for the others: rows are plain data (`RowDef`), each control saves itself.
import { useCallback, useEffect, useState } from 'react';
import { homeDir } from '@tauri-apps/api/path';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import type { Settings } from '@puppet-master/shared';
import { clampAwayMinutes, clampCheckpointsKeep } from '../../../lib/bridge-settings';
import { redactSettings } from '../../../lib/secrets';
import { resolveSettingsFilePath } from '../../../lib/settings';
import { tauri, type AppInstallInfo, type CoordinationStorageInfo } from '../../../lib/tauri';
import { budgetRows } from './BudgetRows';
import { AmbientSettings } from '../blocks/AmbientSettings';
import { AskBarSettingsBlock } from '../blocks/AskBarSettings';
import { AssignIssueSettings } from '../blocks/AssignIssueSettings';
import { BringChatsSettings } from '../blocks/BringChatsSettings';
import { ChatIndexSettings } from '../blocks/ChatIndexSettings';
import { DiagnosticFile } from '../blocks/DiagnosticFile';
import { FocusSettingsBlock } from '../blocks/FocusSettings';
import { LiveRoomRow } from '../blocks/LiveRoomRow';
import { LoopGuardSettingsBlock } from '../blocks/LoopGuardSettings';
import { SharedMemoryToggle } from '../blocks/SharedMemoryToggle';
import { START_AT_SIGN_IN_HINT, START_AT_SIGN_IN_LABEL, StartAtSignInSettings } from '../blocks/StartAtSignInSettings';
import { TeamSettings } from '../blocks/TeamSettings';
import { WhatsNew, WHATS_NEW_LABEL } from '../blocks/WhatsNew';
import { NumberControl, TextControl } from '../controls';
import type { RowDef, SettingsEnv } from '../model';
import { T as SIMPLE } from '../../simple/copy';
import { T as WELCOME } from '../../firstrun/copy';
import { reopenPatch } from '../../firstrun/model';
import { Btn, FieldSelect, InfoRow, Note, SettingBlock, SettingRow, SettingToggle } from '../settings-ui';

function useInstallInfo(): AppInstallInfo | null {
  const [info, setInfo] = useState<AppInstallInfo | null>(null);
  useEffect(() => {
    void tauri.getAppInstallInfo().then(setInfo).catch(() => undefined);
  }, []);
  return info;
}

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

// ---------- project folder ----------

function ProjectFolder({ env }: { env: SettingsEnv }) {
  const value = env.projectPath ?? env.settings.project_path ?? '';
  const save = async (path: string) => {
    env.setError(null);
    try {
      if (env.onProjectPathChange) await env.onProjectPathChange(path);
      else await env.patch({ project_path: path || null });
      env.markSaved();
    } catch (error) {
      env.setError(`Not saved: ${errorText(error)}`);
    }
  };
  const browse = async () => {
    const picked = await openDialog({ directory: true, multiple: false, defaultPath: (await homeDir()) ?? value ?? undefined });
    if (typeof picked === 'string') await save(picked);
  };
  return (
    // A block, not a row: a folder path is long, and a 220px field on the right showed "C:\Users\dev\work".
    <SettingBlock label="Default project folder" description="Where new panes and orchestrator tools start.">
      <div className="st-path">
        <TextControl
          aria-label="Default project folder"
          mono
          title={value}
          value={value}
          placeholder="~/work/my-project"
          normalize={(text) => text.trim()}
          onCommit={save}
        />
        <Btn onClick={() => void browse()}>Browse</Btn>
      </div>
    </SettingBlock>
  );
}

// ---------- coordination storage ----------

function StorageInfo({ env }: { env: SettingsEnv }) {
  const [info, setInfo] = useState<CoordinationStorageInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const refresh = useCallback(async () => {
    try {
      setError(null);
      setInfo(await tauri.getCoordinationStorageInfo());
    } catch (e) {
      setError(errorText(e));
    }
  }, []);
  useEffect(() => {
    void refresh();
  }, [refresh, env.projectPath]);
  return (
    <>
      <InfoRow
        label="Task and activity data"
        description="Tasks, file locks and what happened in each pane are kept per project."
        value={info ? (info.scope === 'project' ? 'Project folder' : 'Shared fallback') : '…'}
      />
      <InfoRow label="Tasks" value={info ? String(info.task_count) : '…'} />
      <InfoRow label="File locks" value={info ? String(info.lock_count) : '…'} />
      <InfoRow
        label="Event log"
        description={info?.event_log_path ?? 'Loading…'}
        value={info ? `${info.event_count} events${info.exists ? '' : ' (created on first write)'}` : '…'}
      />
      <SettingRow label="Storage folder" description={info?.storage_dir ?? 'Loading…'}>
        <Btn onClick={() => void refresh()}>Refresh</Btn>
      </SettingRow>
      {error && <Note role="alert">{error}</Note>}
    </>
  );
}

// ---------- version and updates ----------

function VersionRow({ env }: { env: SettingsEnv }) {
  const info = useInstallInfo();
  const { result, checking, onCheck, onOpenRelease } = env.update;
  const status = result?.updateAvailable && result.latestVersion
    ? `Update available: v${result.latestVersion}.`
    : result?.error
      ? `Could not check for updates: ${result.error}`
      : result?.latestVersion
        ? `You are on the latest release (v${result.latestVersion}).`
        : 'Checked automatically when the app opens.';
  return (
    <SettingRow
      label="Version"
      description={`${info ? `v${info.version}${info.isPackaged ? '' : ' (dev build)'}` : '…'}. ${status}`}
    >
      <Btn disabled={checking} onClick={() => onCheck?.()}>
        {checking ? 'Checking…' : 'Check for updates'}
      </Btn>
      {result?.updateAvailable && (
        <Btn primary onClick={() => onOpenRelease?.()}>
          Download &amp; install
        </Btn>
      )}
    </SettingRow>
  );
}

// ---------- export and uninstall ----------

function exportSettings(settings: Settings) {
  const blob = new Blob([JSON.stringify(redactSettings(settings), null, 2)], { type: 'application/json' });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = 'puppet-master-settings.json';
  anchor.click();
  URL.revokeObjectURL(url);
}

function SettingsFile({ env }: { env: SettingsEnv }) {
  const [path, setPath] = useState('settings.json');
  useEffect(() => {
    void resolveSettingsFilePath().then(setPath);
  }, []);
  return (
    <SettingRow label="Settings file" description={path}>
      <Btn onClick={() => exportSettings(env.settings)}>Export…</Btn>
    </SettingRow>
  );
}

function UninstallRow({ env }: { env: SettingsEnv }) {
  const info = useInstallInfo();
  const [busy, setBusy] = useState(false);
  const run = async () => {
    env.setError(null);
    const ok = await env.confirm({
      title: 'Uninstall Puppet Master?',
      body: 'Puppet Master will close and be removed from this computer. Your app data folder may remain until you delete it yourself.',
      confirmLabel: 'Uninstall',
    });
    if (!ok) return;
    setBusy(true);
    try {
      await tauri.launchUninstall();
    } catch (error) {
      env.setError(errorText(error));
      setBusy(false);
    }
  };
  const description = info
    ? info.uninstallAvailable
      ? `${info.uninstallInstructions}${info.dataDir ? ` App data: ${info.dataDir}` : ''}`
      : 'Only available in an installed build. For a dev build, stop the app and delete the project folder.'
    : 'Loading…';
  return (
    <SettingRow label="Uninstall Puppet Master" description={description}>
      <Btn disabled={busy || !info?.uninstallAvailable} onClick={() => void run()}>
        {busy ? 'Uninstalling…' : 'Uninstall…'}
      </Btn>
    </SettingRow>
  );
}

// ---------- the rows ----------

export const generalRows: RowDef[] = [
  { id: 'project-folder', label: 'Default project folder', hint: 'Where new panes and orchestrator tools start.', keywords: 'workspace directory browse path', group: 'Project', render: (env) => <ProjectFolder env={env} /> },
  {
    id: 'start-at-sign-in',
    label: START_AT_SIGN_IN_LABEL,
    hint: START_AT_SIGN_IN_HINT,
    keywords: 'autostart auto start startup boot login sign in windows launch tray companion reboot restart',
    group: 'Startup',
    render: (env) => <StartAtSignInSettings onSaved={env.markSaved} />,
  },
  {
    id: 'theme',
    label: 'Theme',
    hint: 'Dark or light.',
    keywords: 'appearance dark light',
    group: 'Appearance',
    render: (env) => (
      <SettingRow label="Theme" description="Dark or light.">
        <FieldSelect
          aria-label="Theme"
          value={env.settings.theme ?? 'dark'}
          onChange={(e) => void env.patch({ theme: e.target.value as Settings['theme'] })}
        >
          <option value="dark">Dark</option>
          <option value="light">Light</option>
        </FieldSelect>
      </SettingRow>
    ),
  },
  {
    id: 'simple-mode',
    label: SIMPLE.settingLabel,
    hint: SIMPLE.settingHint,
    keywords: 'simple plain easy beginner fewer buttons words look non-technical',
    group: 'Appearance',
    labs: { name: 'Simple mode', about: 'Fewer buttons and plain words, for people who do not write code.' },
    render: (env) => (
      <SettingToggle
        label={SIMPLE.settingLabel}
        description={SIMPLE.settingHint}
        checked={env.settings.simple_mode ?? false}
        onChange={(value) => void env.patch({ simple_mode: value })}
      />
    ),
  },
  {
    id: 'welcome',
    label: WELCOME.settingLabel,
    hint: WELCOME.settingHint,
    keywords: 'welcome first run tour setup onboarding start again',
    group: 'Appearance',
    render: (env) => (
      <SettingRow label={WELCOME.settingLabel} description={WELCOME.settingHint}>
        <Btn onClick={() => void env.patch(reopenPatch())}>{WELCOME.settingButton}</Btn>
      </SettingRow>
    ),
  },
  {
    id: 'animated-tray',
    label: 'Animated tray icon',
    hint: 'The tray icon shows a moving dot pattern while agents are working. Turn this off for a still icon.',
    keywords: 'ambient motion menu bar',
    group: 'Appearance',
    render: (env) => <AmbientSettings onSaved={env.markSaved} />,
  },
  {
    id: 'confirm-close',
    label: 'Ask before closing a working agent',
    hint: 'Closing a pane window, closing a pane in the cockpit, or quitting from the tray asks first while an agent is working.',
    keywords: 'confirm quit close pane running',
    group: 'While agents work',
    render: (env) => (
      <SettingToggle
        label="Ask before closing a working agent"
        description="Closing a pane window, closing a pane in the cockpit, or quitting from the tray asks first while an agent is working."
        checked={env.settings.confirm_close_running_pane ?? true}
        onChange={(value) => void env.patch({ confirm_close_running_pane: value })}
      />
    ),
  },
  {
    id: 'loop-guard',
    label: 'Warn when an agent seems stuck',
    hint: 'Shows a card when an agent repeats the same error or the same steps, or works for a long time with nothing new.',
    keywords: 'loop guard stuck warning warnings sensitivity repeats quiet too long global shortcut stop all agents panic',
    group: 'While agents work',
    render: (env) => <LoopGuardSettingsBlock onSaved={env.markSaved} />,
  },
  {
    id: 'away-minutes',
    label: 'Away digest after',
    hint: 'After this long without keyboard or mouse, Puppet Master gathers what your agents did and shows it when you come back. 0 turns it off.',
    keywords: 'away minutes digest while you were away idle',
    group: 'While agents work',
    render: (env) => (
      <SettingRow
        label="Away digest after"
        description="After this long without keyboard or mouse, Puppet Master gathers what your agents did and shows it when you come back. 0 turns it off."
      >
        <NumberControl
          aria-label="Away digest after (minutes)"
          min={0}
          max={1440}
          value={env.settings.away_minutes ?? 5}
          normalize={clampAwayMinutes}
          suffix="min"
          onCommit={(minutes) => env.patch({ away_minutes: minutes })}
        />
      </SettingRow>
    ),
  },
  ...budgetRows,
  {
    id: 'focus-time',
    label: 'Focus Time',
    hint: 'How long a Focus runs by default, and whether cards that wait for you go quiet during it.',
    keywords: 'focus time default length minutes quiet blocking cards wait make during held inbox phone telegram buzz pause',
    group: 'Focus Time',
    render: (env) => <FocusSettingsBlock onSaved={env.markSaved} />,
  },
  {
    id: 'ask-bar',
    label: 'Ask bar',
    hint: 'Open a small bar from anywhere in Windows to ask an agent a quick question. Off by default.',
    keywords: 'ask bar shortcut quick question default agent clipboard attach global',
    group: 'Ask bar',
    labs: { name: 'Ask bar', about: 'A small bar you open from anywhere in Windows to ask an agent a quick question.' },
    render: (env) => <AskBarSettingsBlock onSaved={env.markSaved} />,
  },
  {
    id: 'checkpoints',
    label: 'Checkpoints',
    hint: "Save your project's files at the start and end of every agent turn, so you can undo a turn from the History tab. Works in any folder, with any agent. Needs Git installed.",
    keywords: 'undo history turn restore git',
    group: 'Undo and memory',
    render: (env) => (
      <SettingToggle
        label="Checkpoints"
        description="Save your project's files at the start and end of every agent turn, so you can undo a turn from the History tab. Works in any folder, with any agent. Needs Git installed."
        checked={env.settings.checkpoints_enabled ?? true}
        onChange={(value) => void env.patch({ checkpoints_enabled: value })}
      />
    ),
  },
  {
    id: 'checkpoints-keep',
    label: 'Keep last N checkpoints',
    hint: 'Per agent. Older checkpoints are deleted.',
    keywords: 'checkpoints prune',
    group: 'Undo and memory',
    render: (env) => (
      <SettingRow label="Keep last N checkpoints" description="Per agent. Older checkpoints are deleted.">
        <NumberControl
          aria-label="Keep last N checkpoints"
          min={5}
          max={500}
          step={5}
          disabled={env.settings.checkpoints_enabled === false}
          value={env.settings.checkpoints_keep ?? 50}
          normalize={clampCheckpointsKeep}
          onCommit={(keep) => env.patch({ checkpoints_keep: keep })}
        />
      </SettingRow>
    ),
  },
  {
    id: 'shared-memory',
    label: 'Shared project memory',
    hint: 'Every agent in a project reads one shared memory file. Turn off to remove it from agent files.',
    keywords: 'memory claude.md agents.md puppet-master memory.md file',
    group: 'Undo and memory',
    render: (env) => <SharedMemoryToggle onSaved={env.markSaved} />,
  },
  {
    id: 'storage',
    label: 'Task and activity data',
    hint: 'Tasks, file locks and what happened in each pane are kept per project.',
    keywords: 'coordination storage tasks locks events pane events log directory folder refresh',
    group: 'Storage',
    render: (env) => <StorageInfo env={env} />,
  },
  {
    id: 'chat-search-index',
    label: 'Search index of my chats',
    hint: 'Keeps redacted text of your chats so Ctrl+Shift+F can find it. It stays on this computer. Clear it any time; your chats are not touched.',
    keywords: 'keep keeps clear search index chats find ctrl shift f palette history privacy forget saved text',
    group: 'Storage',
    render: (env) => <ChatIndexSettings env={env} />,
  },
  {
    id: 'bring-old-chats',
    label: 'Bring my old chats',
    hint: 'Read the chats you had in Claude Code, Codex and other agents on this PC, and add them to Ctrl+Shift+F search. Nothing leaves this computer. Remove them any time.',
    keywords: 'import old chats claude codex opencode history bring',
    group: 'Storage',
    render: (env) => <BringChatsSettings env={env} />,
  },
  {
    id: 'missions',
    label: 'Missions (Preview)',
    hint: 'Give agents a goal and the checks it must pass. They work in turns in a separate copy until the checks pass, within a budget. Nothing merges on its own.',
    keywords: 'mission missions labs preview goal checks loop until tests pass backup agents',
    group: 'Labs',
    labs: { name: 'Missions', about: 'Agents work in a separate copy, turn by turn, until your checks pass.' },
    render: (env) => (
      <SettingToggle
        label="Missions (Preview)"
        description="Give agents a goal and the checks it must pass. They work in turns in a separate copy until the checks pass, within a budget. Every agent works in a visible pane. Nothing merges on its own."
        checked={env.settings.missions_enabled ?? false}
        onChange={(value) => void env.patch({ missions_enabled: value })}
      />
    ),
  },
  {
    id: 'assign-issue',
    label: 'Assign an issue',
    hint: 'Label a GitHub issue and an agent works on it here.',
    keywords: 'github issue label puppet draft pull request',
    group: 'Labs',
    labs: { name: 'Assign an issue', about: 'Start an agent on a GitHub issue when someone on your list labels it.' },
    render: (env) => <AssignIssueSettings onSaved={env.markSaved} projectPath={env.projectPath} />,
  },
  {
    id: 'night-shift',
    label: 'Night Shift',
    hint: 'Plan tasks for tonight. Agents work on them in separate copies while you sleep, and you keep or throw away each one in the morning.',
    keywords: 'night shift tonight overnight morning sleep labs tasks backup agents',
    group: 'Labs',
    labs: { name: 'Night Shift', about: 'Agents work through your tasks overnight, in separate copies.' },
    render: (env) => (
      <SettingToggle
        label="Night Shift"
        description="Plan tasks for tonight from the + menu. Agents work on them in separate copies while you sleep, with your tests as the check. Nothing merges until you press Keep in the morning."
        checked={env.settings.night_shift_enabled ?? false}
        onChange={(value) => void env.patch({ night_shift_enabled: value })}
      />
    ),
  },
  { id: 'team', label: 'Team', hint: 'Tell teammates when your agent pushes; get a card when theirs does.', keywords: 'team heads-up co-work teammates push pull merge labs shared folder', group: 'Labs', labs: { name: 'Team', about: 'Tell teammates when your agent pushes, and see when theirs does.' }, render: (env) => <TeamSettings onSaved={env.markSaved} /> },
  { id: 'live-room', label: 'Live Room', hint: 'Let people with a link watch a chat. Pick the relay address, how long and how many.', keywords: 'live room share stream watch link relay viewers broadcast default length', group: 'Live Room', render: (env) => <LiveRoomRow onSaved={env.markSaved} /> },
  { id: 'version', label: 'Version', hint: 'Check for updates and download the installer.', keywords: 'app version update release download install', group: 'About', render: (env) => <VersionRow env={env} /> },
  { id: 'whats-new', label: WHATS_NEW_LABEL, hint: 'The short version of what changed, with a link to each place.', keywords: 'release notes changelog new features changes version update 0.2.0', group: 'About', render: (env) => <WhatsNew env={env} /> },
  { id: 'settings-file', label: 'Settings file', hint: 'Where your settings are saved. Export a copy with keys removed.', keywords: 'export json backup path', group: 'About', render: (env) => <SettingsFile env={env} /> },
  {
    id: 'diagnostic-file',
    label: 'Save a diagnostic file',
    hint: 'One .zip with your settings (keys removed), recent app messages and recordings of runs that failed. Nothing is sent anywhere.',
    keywords: 'diagnostic bug report support logs recording flight zip help',
    group: 'About',
    render: (env) => <DiagnosticFile onSaved={env.markSaved} />,
  },
  { id: 'uninstall', label: 'Uninstall Puppet Master', hint: 'Remove the app from this computer.', keywords: 'remove delete', danger: true, render: (env) => <UninstallRow env={env} /> },
];
