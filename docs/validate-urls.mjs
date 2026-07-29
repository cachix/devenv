#!/usr/bin/env node

/**
 * Treat old-urls.txt as the public compatibility contract for the docs site.
 *
 * Every legacy URL must either:
 *   1. still exist in the built site, or
 *   2. follow one or more deployed redirect rules to a real page.
 *
 * The validator reads dist/_redirects rather than public/_redirects so a build
 * that accidentally drops the redirect file also fails.
 */

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { extname, join, relative, sep } from 'node:path';

const root = import.meta.dirname;
const dist = join(root, 'dist');
const manifestFile = join(root, 'old-urls.txt');
const builtRedirectsFile = join(dist, '_redirects');
const origin = 'https://devenv.sh';
const redirectStatuses = new Set([301, 302, 307, 308]);

function fail(message) {
  console.error(message);
  process.exit(1);
}

function walk(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    return entry.isDirectory() ? walk(path) : [path];
  });
}

function routeFor(file) {
  const path = relative(dist, file).split(sep).join('/');
  if (path === 'index.html') return '/';
  if (path.endsWith('/index.html')) return `/${path.slice(0, -'index.html'.length)}`;
  return `/${path}`;
}

function normalizePath(pathname) {
  if (pathname === '/') return '/';
  return pathname.replace(/\/+$/, '') || '/';
}

function escapeRegex(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

if (!existsSync(dist)) {
  fail('dist/ does not exist. Run npm run build first.');
}
if (!existsSync(builtRedirectsFile)) {
  fail('dist/_redirects does not exist. The deployed build would lose all redirect rules.');
}

const builtRoutes = new Set(
  walk(dist)
    .filter((file) => file.endsWith('.html'))
    .map(routeFor)
    .map(normalizePath),
);

function isServed(pathname) {
  const normalized = normalizePath(pathname);
  if (builtRoutes.has(normalized)) return true;

  // Support a legacy URL that intentionally targets a built non-HTML asset.
  let decoded;
  try {
    decoded = decodeURIComponent(pathname);
  } catch {
    return false;
  }
  if (!extname(decoded)) return false;
  return existsSync(join(dist, decoded.replace(/^\/+/, '')));
}

function parseLegacyManifest() {
  const entries = [];
  const seen = new Map();
  const lines = readFileSync(manifestFile, 'utf8').split('\n');

  for (const [index, rawLine] of lines.entries()) {
    const value = rawLine.trim();
    if (!value || value.startsWith('#')) continue;

    let url;
    try {
      url = new URL(value);
    } catch {
      fail(`old-urls.txt:${index + 1}: malformed URL "${value}"`);
    }
    if (url.origin !== origin) {
      fail(`old-urls.txt:${index + 1}: expected a ${origin} URL, got "${value}"`);
    }
    if (url.search || url.hash) {
      fail(`old-urls.txt:${index + 1}: legacy URLs must not contain queries or fragments`);
    }

    const canonical = `${origin}${normalizePath(url.pathname)}`;
    if (seen.has(canonical)) {
      fail(
        `old-urls.txt:${index + 1}: duplicate of line ${seen.get(canonical)} (${canonical})`,
      );
    }
    seen.set(canonical, index + 1);
    entries.push({ url: value, path: normalizePath(url.pathname) });
  }

  if (entries.length === 0) fail('old-urls.txt contains no legacy URLs.');
  return entries;
}

function parseRedirectRules() {
  const rules = [];
  const lines = readFileSync(builtRedirectsFile, 'utf8').split('\n');

  for (const [index, rawLine] of lines.entries()) {
    const value = rawLine.trim();
    if (!value || value.startsWith('#')) continue;

    const [from, to, rawStatus = '302', ...extra] = value.split(/\s+/);
    if (!from || !to || extra.length > 0) {
      fail(`dist/_redirects:${index + 1}: malformed rule "${value}"`);
    }

    const status = Number(rawStatus);
    if (!redirectStatuses.has(status)) {
      fail(
        `dist/_redirects:${index + 1}: ${status} is not a permanent or temporary redirect status`,
      );
    }

    const normalizedFrom = normalizePath(from);
    const pieces = normalizedFrom.split('*');
    const pattern = new RegExp(
      `^${pieces.map(escapeRegex).join('(.*)')}/?$`,
    );
    rules.push({
      from,
      to,
      status,
      pattern,
      splatCount: pieces.length - 1,
      line: index + 1,
    });
  }

  return rules;
}

const legacyUrls = parseLegacyManifest();
const redirectRules = parseRedirectRules();

function matchingRedirect(pathname) {
  for (const rule of redirectRules) {
    const match = normalizePath(pathname).match(rule.pattern);
    if (!match) continue;

    let destination = rule.to;
    for (let index = 0; index < rule.splatCount; index++) {
      const placeholder = index === 0 ? ':splat' : `:splat${index + 1}`;
      destination = destination.replaceAll(placeholder, match[index + 1] ?? '');
    }
    return { rule, destination };
  }
  return null;
}

function resolveLegacyPath(startPath) {
  const chain = [];
  const visited = new Set();
  let current = normalizePath(startPath);

  for (let hop = 0; hop < 16; hop++) {
    if (isServed(current)) {
      return { ok: true, chain, final: current, external: false };
    }
    if (visited.has(current)) {
      return { ok: false, chain, reason: `redirect loop at ${current}` };
    }
    visited.add(current);

    const match = matchingRedirect(current);
    if (!match) {
      return { ok: false, chain, reason: `${current} is neither built nor redirected` };
    }

    let destination;
    try {
      destination = new URL(match.destination, origin);
    } catch {
      return {
        ok: false,
        chain,
        reason: `invalid destination "${match.destination}" on redirect line ${match.rule.line}`,
      };
    }

    chain.push({
      from: current,
      to: destination.href,
      status: match.rule.status,
      line: match.rule.line,
    });
    if (destination.origin !== origin) {
      return { ok: true, chain, final: destination.href, external: true };
    }
    current = normalizePath(destination.pathname);
  }

  return { ok: false, chain, reason: 'more than 16 redirect hops' };
}

// Exact redirect rules must never point at a missing internal destination,
// even when their source predates the production sitemap snapshot.
const brokenRules = [];
for (const rule of redirectRules.filter((candidate) => candidate.splatCount === 0)) {
  let destination;
  try {
    destination = new URL(rule.to, origin);
  } catch {
    brokenRules.push(`line ${rule.line}: invalid destination "${rule.to}"`);
    continue;
  }
  if (destination.origin !== origin) continue;

  const result = resolveLegacyPath(destination.pathname);
  if (!result.ok) {
    brokenRules.push(`line ${rule.line}: ${rule.from} → ${rule.to}: ${result.reason}`);
  }
}

let direct = 0;
let redirected = 0;
let external = 0;
const missing = [];

for (const legacy of legacyUrls) {
  if (isServed(legacy.path)) {
    direct++;
    continue;
  }

  const result = resolveLegacyPath(legacy.path);
  if (!result.ok) {
    missing.push(`${legacy.url}: ${result.reason}`);
    continue;
  }
  redirected++;
  if (result.external) external++;
}

console.log('Legacy URL compatibility contract');
console.log(`  Manifest URLs:       ${legacyUrls.length}`);
console.log(`  Served directly:     ${direct}`);
console.log(`  Resolved redirects:  ${redirected}`);
console.log(`  Redirect rules:      ${redirectRules.length}`);
if (external > 0) console.log(`  External redirects:  ${external}`);

if (brokenRules.length > 0 || missing.length > 0) {
  if (brokenRules.length > 0) {
    console.error(`\nBroken redirect rules (${brokenRules.length}):`);
    for (const failure of brokenRules) console.error(`  ${failure}`);
  }
  if (missing.length > 0) {
    console.error(`\nUnpreserved legacy URLs (${missing.length}):`);
    for (const failure of missing) console.error(`  ${failure}`);
  }
  process.exit(1);
}

console.log(`\nAll ${legacyUrls.length} legacy URLs resolve successfully.`);
