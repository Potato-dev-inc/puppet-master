#!/usr/bin/env node
/**
 * Upload desktop installers to the matching GitHub release.
 *
 * Windows: bundle/nsis/*.exe, bundle/msi/*.msi
 * macOS:   bundle/dmg/*.dmg (e.g. Puppet.Master_0.1.2_aarch64.dmg)
 *
 * Only files whose names contain _{version}_ are uploaded (0.1.2 → _0.1.2_).
 * Tag convention: 0.1.2 → release tag 0-1-2
 *
 * Requires: GitHub CLI (`gh`) authenticated (`gh auth login`).
 *
 * Env:
 *   SKIP_RELEASE_UPLOAD=1     skip upload (build still succeeds)
 *   RELEASE_UPLOAD_REQUIRED=1 fail the script when upload cannot run
 *   GITHUB_REPO=owner/repo     default: from root package.json
 *   RELEASE_TAG=0-1-2          override tag (default: derived from version)
 */
import { spawnSync } from 'node:child_process';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), '..');
const bundleRoot = join(repoRoot, 'packages', 'app', 'src-tauri', 'target', 'release', 'bundle');

function readJson(path) {
  return JSON.parse(readFileSync(path, 'utf8'));
}

function releaseTagFromVersion(version) {
  return String(version).trim().replace(/^v/i, '').split('.').join('-');
}

function resolveRepo() {
  if (process.env.GITHUB_REPO?.trim()) {
    return process.env.GITHUB_REPO.trim();
  }
  const pkg = readJson(join(repoRoot, 'package.json'));
  const url = pkg.repository?.url ?? '';
  const match = /github\.com[:/](.+?)(?:\.git)?$/i.exec(url);
  return match?.[1] ?? 'Potato-dev-inc/puppet-master';
}

function resolveVersion() {
  const tauriConf = join(repoRoot, 'packages', 'app', 'src-tauri', 'tauri.conf.json');
  if (existsSync(tauriConf)) {
    return readJson(tauriConf).version;
  }
  return readJson(join(repoRoot, 'package.json')).version;
}

function versionTokenFor(version) {
  return `_${String(version).trim().replace(/^v/i, '')}_`;
}

function installerNameMatchesVersion(fileName, version) {
  return fileName.includes(versionTokenFor(version));
}

function collectMatchingFiles(dir, version, extensions) {
  if (!existsSync(dir)) return [];
  const lowerExtensions = extensions.map((ext) => ext.toLowerCase());
  return readdirSync(dir)
    .filter((name) => {
      const lower = name.toLowerCase();
      return lowerExtensions.some((ext) => lower.endsWith(ext)) && installerNameMatchesVersion(name, version);
    })
    .map((name) => join(dir, name));
}

function findReleaseAssets(version) {
  return [
    ...collectMatchingFiles(join(bundleRoot, 'nsis'), version, ['.exe']),
    ...collectMatchingFiles(join(bundleRoot, 'msi'), version, ['.msi']),
    ...collectMatchingFiles(join(bundleRoot, 'dmg'), version, ['.dmg']),
  ].sort();
}

function gh(args) {
  const cmd = process.platform === 'win32' ? 'gh.exe' : 'gh';
  const result = spawnSync(cmd, args, {
    stdio: 'inherit',
    shell: false,
    env: process.env,
  });
  if (result.error) {
    return { ok: false, error: result.error };
  }
  if (result.status !== 0) {
    return { ok: false, error: new Error(`gh exited with status ${result.status}`) };
  }
  return { ok: true };
}

function ghCapture(args) {
  const cmd = process.platform === 'win32' ? 'gh.exe' : 'gh';
  const result = spawnSync(cmd, args, {
    encoding: 'utf8',
    shell: false,
    env: process.env,
  });
  if (result.error || result.status !== 0) {
    return null;
  }
  return result.stdout.trim();
}

function fail(message) {
  console.error(`[release-upload] ${message}`);
  process.exit(1);
}

function warn(message) {
  console.error(`[release-upload] ${message}`);
}

function main() {
  if (process.env.SKIP_RELEASE_UPLOAD === '1') {
    warn('SKIP_RELEASE_UPLOAD=1 — skipping GitHub release upload.');
    return;
  }

  const required = process.env.RELEASE_UPLOAD_REQUIRED === '1';
  const version = resolveVersion();
  const tag = process.env.RELEASE_TAG?.trim() || releaseTagFromVersion(version);
  const assets = findReleaseAssets(version);
  if (assets.length === 0) {
    const msg =
      `No release installers for v${version} found under ${bundleRoot}\n` +
      `Expected filenames containing "${versionTokenFor(version)}" in:\n` +
      '  bundle/nsis/*.exe, bundle/msi/*.msi, bundle/dmg/*.dmg';
    if (required) fail(msg);
    warn(`${msg}\nSkipping upload.`);
    return;
  }

  const ghVersion = ghCapture(['--version']);
  if (!ghVersion) {
    const msg =
      'GitHub CLI (gh) is not installed or not on PATH.\n' +
      'Install from https://cli.github.com/ then run: gh auth login';
    if (required) fail(msg);
    warn(`${msg}\nSkipping upload.`);
    return;
  }

  const repo = resolveRepo();

  warn(`Uploading ${assets.length} installer(s) to ${repo} release ${tag} (v${version})…`);
  for (const asset of assets) {
    warn(`  • ${asset}`);
  }

  const releaseExists = ghCapture(['release', 'view', tag, '--repo', repo, '--json', 'tagName']);
  if (!releaseExists) {
    warn(`Release ${tag} not found — creating draft release for v${version}.`);
    const create = gh([
      'release',
      'create',
      tag,
      '--repo',
      repo,
      '--title',
      `Puppet Master ${version}`,
      '--notes',
      `Desktop installers for Puppet Master ${version}.`,
    ]);
    if (!create.ok) {
      if (required) fail(`Could not create release ${tag}.`);
      warn(`Could not create release ${tag}. Skipping upload.`);
      return;
    }
  }

  const upload = gh([
    'release',
    'upload',
    tag,
    ...assets,
    '--repo',
    repo,
    '--clobber',
  ]);
  if (!upload.ok) {
    if (required) fail(`Upload to release ${tag} failed.`);
    warn(`Upload to release ${tag} failed.`);
    process.exit(1);
  }

  warn(`Done. https://github.com/${repo}/releases/tag/${tag}`);
}

main();
