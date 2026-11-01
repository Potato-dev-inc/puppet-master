// @vitest-environment node
// The Artifacts words, the frame's document, the command names, and the fake that behaves like the store.
import { describe, expect, it, vi } from 'vitest';
import {
  ARTIFACT_COPY,
  ARTIFACT_CSP,
  artifactFrameDoc,
  cleanFilter,
  fakeArtifactTransport,
  hasContent,
  sizeLabel,
  tauriArtifactTransport,
  versionLabel,
  type Artifact,
  type ArtifactPreview,
  type FakeArtifact,
} from './potato-artifacts';

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke }));

const file = (over: Partial<FakeArtifact> & Pick<Artifact, 'id' | 'name' | 'bot' | 'kind'>): FakeArtifact => ({
  size: 10,
  created: 1_700_000_000_000,
  version: 1,
  file: `${over.id}.bin`,
  ...over,
});

describe('artifact words and helpers', () => {
  it('has the exact sentences of the page', () => {
    expect(ARTIFACT_COPY.empty).toBe('No artifacts found.');
    expect(ARTIFACT_COPY.unsupported).toBe("Preview isn't available for this file type. Download it to view it.");
    expect(ARTIFACT_COPY.removeAsk).toBe("This can't be undone.");
    expect(ARTIFACT_COPY.isolated).toBe("Preview only. It can't reach your account.");
    expect(ARTIFACT_COPY.saved('plan.md')).toBe('Saved as plan.md. Find it in Artifacts.');
    expect(ARTIFACT_COPY.downloaded('plan.md')).toBe('Saved a copy of plan.md.');
  });

  it('writes sizes in bytes, kilobytes or megabytes', () => {
    expect(sizeLabel(812)).toBe('812 B');
    expect(sizeLabel(4300)).toBe('4.2 KB');
    expect(sizeLabel(1_400_000)).toBe('1.3 MB');
  });

  it('says which version a row is, with its age text', () => {
    expect(versionLabel(file({ id: 'a', name: 'x', bot: 'b', kind: 'text', version: 3 }), '2h ago')).toBe('Version 3 · 2h ago');
  });

  it('keeps only the filters that are set, and trims the search', () => {
    expect(cleanFilter({})).toEqual({});
    expect(cleanFilter({ bot: 'nimbus', kind: 'pdf', search: '  cover  ' })).toEqual({ bot: 'nimbus', kind: 'pdf', search: 'cover' });
    expect(cleanFilter({ bot: '', search: '   ' })).toEqual({});
  });

  it('knows whether a preview has something to draw', () => {
    const base: ArtifactPreview = { artifact: file({ id: 'a', name: 'x', bot: 'b', kind: 'pdf' }), versions: [], text: null, truncated: false, image: null };
    expect(hasContent(base)).toBe(false);
    expect(hasContent({ ...base, text: '' })).toBe(true);
    expect(hasContent({ ...base, image: 'data:image/png;base64,AQID' })).toBe(true);
  });
});

describe('the frame of an HTML artifact', () => {
  it('blocks every network load and never allows scripts', () => {
    expect(ARTIFACT_CSP).toContain("default-src 'none'");
    expect(ARTIFACT_CSP).not.toMatch(/script-src|https?:/);
  });

  it('puts the policy into the head of a full page', () => {
    const doc = artifactFrameDoc('<!doctype html><html><head><title>t</title></head><body>hi</body></html>');
    expect(doc.indexOf('Content-Security-Policy')).toBeLessThan(doc.indexOf('<title>'));
    expect(doc.match(/Content-Security-Policy/g)?.length).toBe(1);
  });

  it('makes a head for a page without one, and wraps a plain fragment', () => {
    expect(artifactFrameDoc('<html lang="en"><body>x</body></html>')).toMatch(/<html lang="en"><head><meta http-equiv="Content-Security-Policy"/);
    const fragment = artifactFrameDoc('<p>hello</p>');
    expect(fragment.startsWith('<!doctype html>')).toBe(true);
    expect(fragment).toContain('<body><p>hello</p></body>');
  });
});

describe('the Tauri commands', () => {
  it('calls the commands by their names, with the arguments the Rust side reads', async () => {
    invoke.mockReset();
    invoke.mockResolvedValue([]);
    await tauriArtifactTransport.list();
    expect(invoke).toHaveBeenLastCalledWith('potato_artifact_list', { filter: null });
    await tauriArtifactTransport.list({ bot: 'nimbus', kind: 'image', search: 'x' });
    expect(invoke).toHaveBeenLastCalledWith('potato_artifact_list', { filter: { bot: 'nimbus', kind: 'image', search: 'x' } });
    await tauriArtifactTransport.open('nimbus', 'a-1');
    expect(invoke).toHaveBeenLastCalledWith('potato_artifact_open', { botId: 'nimbus', id: 'a-1' });
    await tauriArtifactTransport.saveMessage('nimbus', 'm-1');
    expect(invoke).toHaveBeenLastCalledWith('potato_artifact_save_message', { botId: 'nimbus', messageId: 'm-1', name: null });
    await tauriArtifactTransport.download('nimbus', 'a-1', 'C:\\Users\\me\\plan.md');
    expect(invoke).toHaveBeenLastCalledWith('potato_artifact_export', { botId: 'nimbus', id: 'a-1', dest: 'C:\\Users\\me\\plan.md' });
    await tauriArtifactTransport.reveal('nimbus', 'a-1');
    expect(invoke).toHaveBeenLastCalledWith('potato_artifact_reveal', { botId: 'nimbus', id: 'a-1' });
    await tauriArtifactTransport.remove('nimbus', 'a-1');
    expect(invoke).toHaveBeenLastCalledWith('potato_artifact_delete', { botId: 'nimbus', id: 'a-1' });
  });
});

describe('the fake store', () => {
  const seed = () =>
    fakeArtifactTransport({
      items: [
        file({ id: 'a-1', name: 'plan.md', bot: 'nimbus', kind: 'markdown', version: 1, created: 1_000, content: 'one' }),
        file({ id: 'a-2', name: 'Plan.md', bot: 'nimbus', kind: 'markdown', version: 2, created: 2_000, content: 'two' }),
        file({ id: 'a-3', name: 'chart.png', bot: 'tango', kind: 'image', created: 3_000 }),
        file({ id: 'a-4', name: 'page.html', bot: 'nimbus', kind: 'html', created: 4_000, content: '<p>x</p>' }),
      ],
    });

  it('lists one row per name, the newest version, newest first', async () => {
    const rows = await seed().list();
    expect(rows.map((r) => r.id)).toEqual(['a-4', 'a-3', 'a-2']);
    expect(rows.find((r) => r.name.toLowerCase() === 'plan.md')).toMatchObject({ id: 'a-2', versions: 2 });
  });

  it('filters by bot, kind and part of the name', async () => {
    const t = seed();
    expect((await t.list({ bot: 'tango' })).map((r) => r.id)).toEqual(['a-3']);
    expect((await t.list({ kind: 'html' })).map((r) => r.id)).toEqual(['a-4']);
    expect((await t.list({ search: 'PLA' })).map((r) => r.id)).toEqual(['a-2']);
    expect(await t.list({ kind: 'pdf' })).toEqual([]);
  });

  it('opens a version with its text and every version of its name, newest first', async () => {
    const preview = await seed().open('nimbus', 'a-1');
    expect(preview.artifact.id).toBe('a-1');
    expect(preview.text).toBe('one');
    expect(preview.versions.map((v) => v.version)).toEqual([2, 1]);
  });

  it('gives an image a data address and a text file no image', async () => {
    const t = seed();
    expect((await t.open('tango', 'a-3')).image).toMatch(/^data:image\/png;base64,/);
    expect((await t.open('nimbus', 'a-1')).image).toBeNull();
  });

  it('saves a message as a Markdown file and names it', async () => {
    const t = seed();
    const saved = await t.saveMessage('nimbus', 'm-9');
    expect(saved).toMatchObject({ name: 'message.md', kind: 'markdown', version: 1 });
    expect(await t.saveMessage('nimbus', 'm-10', 'plan')).toMatchObject({ name: 'plan.md' });
  });

  it('deletes one version and says when the file is gone', async () => {
    const t = seed();
    await t.remove('nimbus', 'a-2');
    expect((await t.list({ search: 'plan' }))[0]).toMatchObject({ id: 'a-1', versions: 1 });
    await expect(t.remove('nimbus', 'a-2')).rejects.toThrow('POTATO_ARTIFACT_NOT_FOUND');
  });

  it('answers a failure it was told to give, with its code', async () => {
    const t = fakeArtifactTransport({ items: [], fail: { download: 'The disk is full.' } });
    await expect(t.download('nimbus', 'a-1', 'x')).rejects.toThrow('POTATO_ARTIFACT_INVALID: The disk is full.');
  });

  it('records every call in order', async () => {
    const t = seed();
    await t.list({ bot: 'nimbus' });
    await t.open('nimbus', 'a-1');
    expect(t.calls.map((c) => c.method)).toEqual(['list', 'open']);
  });
});
