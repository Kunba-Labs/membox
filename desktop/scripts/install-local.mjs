#!/usr/bin/env node
// Local app installer (macOS) — inbox2's mechanism.
//
// Builds the Tauri app for THIS machine, signs it with a stable identity, and
// installs the .app into /Applications. Two identities live side by side —
// separate Dock icons, separate data dirs, separate MCP endpoints, separate
// iCloud sync folders — and can run at the same time:
//
//   yarn install:app        membox      (com.membox.desktop      — the one you use daily)
//   yarn install:app:dev    membox Dev  (com.membox.desktop.dev  — the one you break)
//
// The dev identity is the SAME one `yarn dev` / `bin/dev` uses, so the bundled
// "membox Dev" app and the dev loop share one library.
//
// Signing: Developer ID if present, else Apple Development; override with
// APPLE_SIGNING_IDENTITY (exact string, or "-" for ad-hoc). Notarization is
// skipped on purpose — a locally built, locally signed app launches fine.

import { execFileSync } from 'node:child_process';
import { readdirSync, rmSync, cpSync, existsSync } from 'node:fs';
import { dirname, resolve, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { platform } from 'node:os';

const __dirname = dirname(fileURLToPath(import.meta.url));
const DESKTOP_DIR = resolve(__dirname, '..');
// Workspace target dir (Cargo.toml at the repo root).
const BUNDLE_MACOS = resolve(DESKTOP_DIR, '../target/release/bundle/macos');
const APPLICATIONS = '/Applications';

const IDENTITY_PREFERENCE = [/^Developer ID Application:/, /^Apple Development:/];

function availableIdentities() {
  try {
    const out = execFileSync('security', ['find-identity', '-v', '-p', 'codesigning'], { encoding: 'utf8' });
    return [...out.matchAll(/^\s*\d+\)\s+\S+\s+"([^"]+)"/gm)].map((m) => m[1]);
  } catch {
    return [];
  }
}

function resolveSigningIdentity() {
  const available = availableIdentities();
  const requested = process.env.APPLE_SIGNING_IDENTITY;
  if (requested === '-') return '-';
  if (requested) {
    if (!available.includes(requested)) {
      console.error(`error: APPLE_SIGNING_IDENTITY "${requested}" is not in the keychain.\n  Available:${available.map((i) => `\n    ${i}`).join('') || ' (none)'}`);
      process.exit(1);
    }
    return requested;
  }
  for (const p of IDENTITY_PREFERENCE) {
    const found = available.find((i) => p.test(i));
    if (found) return found;
  }
  console.error('error: no code-signing identity in the keychain. Set APPLE_SIGNING_IDENTITY="-" for ad-hoc.');
  process.exit(1);
}

if (platform() !== 'darwin') {
  console.error('install-local.mjs supports macOS only.');
  process.exit(1);
}

const isDev = process.argv.includes('--dev');
const plan = isDev
  ? { appName: 'membox Dev.app', identifier: 'com.membox.desktop.dev', configArg: 'src-tauri/tauri.dev.conf.json' }
  : { appName: 'membox.app', identifier: 'com.membox.desktop', configArg: null };

const signingIdentity = resolveSigningIdentity();
const log = (m) => console.log(`\x1b[36m•\x1b[0m ${m}`);

// Refuse to clobber a running copy — macOS keeps the binary mapped.
try {
  const out = execFileSync('pgrep', ['-f', `/Applications/${plan.appName}/Contents/MacOS/`], { encoding: 'utf8' });
  if (out.trim()) {
    console.error(`error: ${plan.appName} is running (pid ${out.trim().split('\n').join(', ')}). Quit it first.`);
    process.exit(1);
  }
} catch { /* nothing running — good */ }

log(`building ${plan.appName} (${plan.identifier})${signingIdentity === '-' ? ' [ad-hoc]' : ` signed: ${signingIdentity}`}`);
const args = ['tauri', 'build', '--bundles', 'app'];
if (plan.configArg) args.push('-c', plan.configArg);
// The daily app is the prod build: same updater config and version scheme as a
// CI release, so the next release updates it in place. It skips the updater
// artifacts — only CI signs those, and they need the key.
if (!isDev) {
  const version = execFileSync(resolve(DESKTOP_DIR, '../bin/version'), { encoding: 'utf8' }).trim();
  args.push('-c', 'src-tauri/tauri.release.conf.json', '--config', JSON.stringify({ version, bundle: { createUpdaterArtifacts: false } }));
}
execFileSync('yarn', args, { stdio: 'inherit', cwd: DESKTOP_DIR, env: { ...process.env, APPLE_SIGNING_IDENTITY: signingIdentity } });

const built = join(BUNDLE_MACOS, plan.appName);
if (!existsSync(built)) {
  const found = existsSync(BUNDLE_MACOS) ? readdirSync(BUNDLE_MACOS).filter((n) => n.endsWith('.app')) : [];
  console.error(`error: expected ${plan.appName} under ${BUNDLE_MACOS}, found: ${found.join(', ') || '(none)'}`);
  process.exit(1);
}

const dest = join(APPLICATIONS, plan.appName);
if (existsSync(dest)) {
  log(`removing existing ${dest}`);
  rmSync(dest, { recursive: true, force: true });
}
log(`installing → ${dest}`);
cpSync(built, dest, { recursive: true });
// The build-tree copy carries the same bundle id; leaving it makes Spotlight
// offer two identical apps, one of them stale.
rmSync(built, { recursive: true, force: true });
try { execFileSync('xattr', ['-dr', 'com.apple.quarantine', dest], { stdio: 'ignore' }); } catch {}

log(`done — ${dest}`);
log(`library: ~/Library/Application Support/${plan.identifier}/`);
