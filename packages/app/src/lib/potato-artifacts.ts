// Potato Bot artifacts: the files a bot made or received, for the Artifacts page and "Save as file" in a thread. The names
// mirror `potato/artifacts.rs` and `commands/potato_artifacts.rs` in camelCase. Desktop only. Nothing here draws.
// `fakeArtifactTransport` is the in-memory twin for tests. HTML is never run here: the page shows it in a sandboxed frame
// with no scripts (`artifactFrameDoc`).
import { invoke } from '@tauri-apps/api/core';

export type ArtifactKind = 'image' | 'text' | 'markdown' | 'html' | 'pdf' | 'other';

export interface Artifact {
  id: string;
  /** The name, cleaned: a label, never a path. */
  name: string;
  bot: string;
  kind: ArtifactKind;
  /** Bytes. */
  size: number;
  /** Epoch milliseconds. */
  created: number;
  /** 1 for the first file with this name, then 2, 3 ... */
  version: number;
  file: string;
}

/** One row of the list: the newest version of a name, and how many versions the name has. */
export interface ArtifactListItem extends Artifact {
  versions: number;
}

/** One version opened: its content when the kind can be shown, and every version of its name, newest first. */
export interface ArtifactPreview {
  artifact: Artifact;
  versions: Artifact[];
  /** Text, Markdown and HTML (the first megabyte). Null for images, PDFs and other files. */
  text: string | null;
  truncated: boolean;
  /** Images only: a data address for an image tag. */
  image: string | null;
}

export interface ArtifactFilter {
  bot?: string;
  kind?: ArtifactKind;
  /** Part of the name, without regard to case. */
  search?: string;
}

export interface ArtifactTransport {
  list(filter?: ArtifactFilter): Promise<ArtifactListItem[]>;
  open(botId: string, id: string): Promise<ArtifactPreview>;
  /** "Save as file" on a chat message. `name` is optional; without it the first line of the message names it. */
  saveMessage(botId: string, messageId: string, name?: string): Promise<Artifact>;
  /** Download / Save as: copies the file to `dest` (a path the person picked). */
  download(botId: string, id: string, dest: string): Promise<void>;
  /** Show in folder: opens the folder with the file selected. */
  reveal(botId: string, id: string): Promise<void>;
  /** Deletes one version. The page asks first. */
  remove(botId: string, id: string): Promise<void>;
}

export const tauriArtifactTransport: ArtifactTransport = {
  list: (filter) => invoke<ArtifactListItem[]>('potato_artifact_list', { filter: filter ?? null }),
  open: (botId, id) => invoke<ArtifactPreview>('potato_artifact_open', { botId, id }),
  saveMessage: (botId, messageId, name) => invoke<Artifact>('potato_artifact_save_message', { botId, messageId, name: name ?? null }),
  download: (botId, id, dest) => invoke<void>('potato_artifact_export', { botId, id, dest }),
  reveal: (botId, id) => invoke<void>('potato_artifact_reveal', { botId, id }),
  remove: (botId, id) => invoke<void>('potato_artifact_delete', { botId, id }),
};

// ---- copy and small helpers ---------------------------------------------------------------------

export const KIND_LABEL: Record<ArtifactKind, string> = {
  image: 'Image',
  text: 'Text',
  markdown: 'Markdown',
  html: 'Web page',
  pdf: 'PDF',
  other: 'File',
};

/** The kinds in the order the filter offers them. */
export const KIND_ORDER: ArtifactKind[] = ['image', 'markdown', 'text', 'html', 'pdf', 'other'];

export const ARTIFACT_COPY = {
  title: 'Artifacts',
  empty: 'No artifacts found.',
  emptyFiltered: 'No artifacts match these filters.',
  search: 'Search artifacts',
  filters: 'Filters',
  allBots: 'All bots',
  allKinds: 'All kinds',
  unsupported: "Preview isn't available for this file type. Download it to view it.",
  isolated: "Preview only. It can't reach your account.",
  download: 'Download',
  reveal: 'Show in folder',
  remove: 'Delete',
  removeAsk: "This can't be undone.",
  cancel: 'Cancel',
  version: 'Version',
  truncated: 'Only the start is shown. Download the file to see all of it.',
  back: 'Back to chat',
  saveAsFile: 'Save as file',
  saved: (name: string) => `Saved as ${name}. Find it in Artifacts.`,
  downloaded: (name: string) => `Saved a copy of ${name}.`,
  deleted: (name: string) => `Deleted ${name}.`,
  asGrid: 'Show as grid',
  asList: 'Show as list',
  loading: 'Loading…',
  pick: 'Pick a file to see it here.',
} as const;

/** Bytes as a short size: `812 B`, `4.2 KB`, `1.3 MB`. */
export function sizeLabel(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/** Whether the preview has something to draw (text or an image). */
export function hasContent(preview: ArtifactPreview): boolean {
  return preview.text !== null || preview.image !== null;
}

/** The filters as the command takes them: absent parts left out, the search trimmed. */
export function cleanFilter(filter: ArtifactFilter): ArtifactFilter {
  const out: ArtifactFilter = {};
  if (filter.bot) out.bot = filter.bot;
  if (filter.kind) out.kind = filter.kind;
  const search = filter.search?.trim();
  if (search) out.search = search;
  return out;
}

/** "Version 3 · 2h ago" for the version select; `when` is the age text the page shows for the version. */
export function versionLabel(artifact: Artifact, when: string): string {
  return `${ARTIFACT_COPY.version} ${artifact.version} · ${when}`;
}

/** The Content-Security-Policy of an artifact's frame: nothing loads, scripts never run, only inline styles and data images. */
export const ARTIFACT_CSP = "default-src 'none'; img-src data:; style-src 'unsafe-inline'; font-src data:; media-src data:; form-action 'none'";

/**
 * The document a sandboxed frame shows for an HTML artifact. The frame has `sandbox=""` (no scripts, no same origin),
 * and this adds a policy on top: nothing can load from the network. The policy goes into the head, or a head is made.
 */
export function artifactFrameDoc(html: string): string {
  const meta = `<meta http-equiv="Content-Security-Policy" content="${ARTIFACT_CSP}">`;
  if (/<head[\s>]/i.test(html)) return html.replace(/<head[^>]*>/i, (open) => `${open}${meta}`);
  if (/<html[\s>]/i.test(html)) return html.replace(/<html[^>]*>/i, (open) => `${open}<head>${meta}</head>`);
  return `<!doctype html><html><head><meta charset="utf-8">${meta}</head><body>${html}</body></html>`;
}

// ---- the fake (tests) ---------------------------------------------------------------------------

export interface FakeArtifact extends Artifact {
  /** The text the preview shows (text, Markdown, HTML). */
  content?: string;
}

export interface FakeArtifactSeed {
  items?: FakeArtifact[];
  /** What a download or a delete answers when it fails: a sentence, thrown. */
  fail?: Partial<Record<'download' | 'remove' | 'saveMessage' | 'reveal', string>>;
}

export interface FakeArtifactTransport extends ArtifactTransport {
  readonly calls: { method: keyof ArtifactTransport; args: unknown[] }[];
}

/** An in-memory store that behaves like the app: versions by name, filters, the preview's version list. */
export function fakeArtifactTransport(seed: FakeArtifactSeed = {}): FakeArtifactTransport {
  const items: FakeArtifact[] = (seed.items ?? []).map((i) => ({ ...i }));
  const calls: FakeArtifactTransport['calls'] = [];
  let counter = items.length;
  const log = <T>(method: keyof ArtifactTransport, args: unknown[], run: () => T): Promise<T> => {
    calls.push({ method, args });
    try {
      return Promise.resolve(run());
    } catch (error) {
      return Promise.reject(error);
    }
  };
  const fail = (method: 'download' | 'remove' | 'saveMessage' | 'reveal') => {
    const sentence = seed.fail?.[method];
    if (sentence) throw new Error(`POTATO_ARTIFACT_INVALID: ${sentence}`);
  };
  const strip = ({ content: _content, ...rest }: FakeArtifact): Artifact => rest;
  const find = (botId: string, id: string): FakeArtifact => {
    const hit = items.find((i) => i.bot === botId && i.id === id);
    if (!hit) throw new Error('POTATO_ARTIFACT_NOT_FOUND: That file is gone. It may have been deleted.');
    return hit;
  };
  const sameName = (a: string, b: string) => a.toLowerCase() === b.toLowerCase();

  return {
    calls,
    list: (filter) =>
      log('list', [filter], () => {
        const f = cleanFilter(filter ?? {});
        const needle = f.search?.toLowerCase();
        const families = new Map<string, FakeArtifact[]>();
        for (const item of items) {
          const key = `${item.bot}\u0000${item.name.toLowerCase()}`;
          families.set(key, [...(families.get(key) ?? []), item]);
        }
        const rows: ArtifactListItem[] = [];
        for (const versions of families.values()) {
          const newest = versions.reduce((a, b) => (b.version > a.version ? b : a));
          if (f.bot && newest.bot !== f.bot) continue;
          if (f.kind && newest.kind !== f.kind) continue;
          if (needle && !newest.name.toLowerCase().includes(needle)) continue;
          rows.push({ ...strip(newest), versions: versions.length });
        }
        return rows.sort((a, b) => b.created - a.created || a.name.localeCompare(b.name));
      }),
    open: (botId, id) =>
      log('open', [botId, id], () => {
        const hit = find(botId, id);
        const versions = items.filter((i) => i.bot === botId && sameName(i.name, hit.name)).sort((a, b) => b.version - a.version).map(strip);
        const text = hit.kind === 'text' || hit.kind === 'markdown' || hit.kind === 'html' ? (hit.content ?? '') : null;
        return { artifact: strip(hit), versions, text, truncated: false, image: hit.kind === 'image' ? 'data:image/png;base64,AQID' : null };
      }),
    saveMessage: (botId, messageId, name) =>
      log('saveMessage', [botId, messageId, name], () => {
        fail('saveMessage');
        const label = name && name.includes('.') ? name : `${name ?? 'message'}.md`;
        const same = items.filter((i) => i.bot === botId && sameName(i.name, label));
        const saved: FakeArtifact = {
          id: `a-fake-${++counter}`,
          name: label,
          bot: botId,
          kind: 'markdown',
          size: 0,
          created: Date.now(),
          version: same.length + 1,
          file: `a-fake-${counter}.md`,
          content: `(saved from ${messageId})`,
        };
        items.push(saved);
        return strip(saved);
      }),
    download: (botId, id, dest) =>
      log('download', [botId, id, dest], () => {
        fail('download');
        find(botId, id);
      }),
    reveal: (botId, id) =>
      log('reveal', [botId, id], () => {
        fail('reveal');
        find(botId, id);
      }),
    remove: (botId, id) =>
      log('remove', [botId, id], () => {
        fail('remove');
        const at = items.findIndex((i) => i.bot === botId && i.id === id);
        if (at < 0) throw new Error('POTATO_ARTIFACT_NOT_FOUND: That file is gone. It may have been deleted.');
        items.splice(at, 1);
      }),
  };
}
