#!/usr/bin/env node

/**
 * Validate links in the generated site, including same-site fragments and
 * referenced assets. This deliberately checks dist/ so it sees the routes and
 * heading IDs Astro actually emitted.
 */

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { extname, join, relative, sep } from 'node:path';

const root = import.meta.dirname;
const dist = join(root, 'dist');
const origin = 'https://devenv.sh';

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

function decodeHtml(value) {
  return value
    .replaceAll('&amp;', '&')
    .replaceAll('&quot;', '"')
    .replaceAll('&#39;', "'")
    .replace(/&#x([0-9a-f]+);/gi, (_, hex) => String.fromCodePoint(Number.parseInt(hex, 16)))
    .replace(/&#(\d+);/g, (_, number) => String.fromCodePoint(Number(number)));
}

function targetFile(pathname) {
  let decoded;
  try {
    decoded = decodeURIComponent(pathname);
  } catch {
    return null;
  }

  const local = decoded.replace(/^\/+/, '');
  if (decoded === '/') return join(dist, 'index.html');
  if (decoded === '/404/' && existsSync(join(dist, '404.html'))) {
    return join(dist, '404.html');
  }
  if (decoded.endsWith('/')) return join(dist, local, 'index.html');
  if (extname(decoded)) return join(dist, local);

  const direct = join(dist, local);
  if (existsSync(direct)) return direct;
  return join(direct, 'index.html');
}

if (!existsSync(dist)) {
  console.error('dist/ does not exist. Run npm run build first.');
  process.exit(1);
}

const htmlFiles = walk(dist).filter((file) => file.endsWith('.html'));
const pages = new Map();
for (const file of htmlFiles) {
  const html = readFileSync(file, 'utf8');
  const ids = new Set(
    [...html.matchAll(/\sid=(["'])(.*?)\1/g)].map((match) => decodeHtml(match[2])),
  );
  pages.set(file, { html, ids, route: routeFor(file) });
}

const failures = [];
let checkedLinks = 0;
let checkedFragments = 0;
let checkedAssets = 0;

for (const page of pages.values()) {
  for (const match of page.html.matchAll(/\s(href|src)=(["'])(.*?)\2/g)) {
    const [, attribute, , encodedValue] = match;
    const value = decodeHtml(encodedValue.trim());
    if (
      !value ||
      value.startsWith('data:') ||
      value.startsWith('mailto:') ||
      value.startsWith('tel:') ||
      value.startsWith('javascript:') ||
      value.startsWith('//')
    ) {
      continue;
    }

    let url;
    try {
      url = new URL(value, `${origin}${page.route}`);
    } catch {
      failures.push(`${page.route}: malformed ${attribute}="${value}"`);
      continue;
    }
    if (url.origin !== origin) continue;

    const target = targetFile(url.pathname);
    if (!target || !existsSync(target)) {
      failures.push(`${page.route}: ${attribute}="${value}" does not exist`);
      continue;
    }

    if (attribute === 'src') {
      checkedAssets++;
      continue;
    }
    checkedLinks++;

    if (url.hash && target.endsWith('.html')) {
      const targetPage = pages.get(target);
      let fragment;
      try {
        fragment = decodeURIComponent(url.hash.slice(1));
      } catch {
        failures.push(`${page.route}: malformed fragment in href="${value}"`);
        continue;
      }
      if (!targetPage?.ids.has(fragment)) {
        failures.push(`${page.route}: fragment "#${fragment}" is missing from ${url.pathname}`);
      } else {
        checkedFragments++;
      }
    }
  }
}

if (failures.length > 0) {
  console.error(`Internal link validation failed (${failures.length}):`);
  for (const failure of failures) console.error(`  ${failure}`);
  process.exit(1);
}

console.log(
  `Validated ${checkedLinks} internal links, ${checkedFragments} fragments, and ${checkedAssets} assets across ${htmlFiles.length} pages.`,
);
