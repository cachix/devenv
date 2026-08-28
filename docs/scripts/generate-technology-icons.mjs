import { mkdir, readdir, readFile, unlink, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { technologyRegistry } from '../src/data/technology-registry.ts';
import { technologyIconFilename } from '../src/lib/technology-icon-path.ts';

const root = dirname(fileURLToPath(import.meta.url));
const iconSource = join(root, '..', 'node_modules', 'simple-icons', 'icons');
const output = join(root, '..', 'public', 'technology-icons');
const variants = new Map();

for (const technology of technologyRegistry) {
  if (!technology.icon) continue;
  for (const color of new Set([technology.color, technology.darkColor ?? technology.color])) {
    variants.set(technologyIconFilename(technology.icon, color), { ...technology, color });
  }
}
variants.set(technologyIconFilename('nixos', '#8BB8FF'), { icon: 'nixos', color: '#8BB8FF' });

await mkdir(output, { recursive: true });

let written = 0;
let cached = 0;
for (const [filename, technology] of variants) {
  const source = await readFile(join(iconSource, `${technology.icon}.svg`), 'utf8').catch(() => null);
  if (!source) throw new Error(`Simple Icons does not contain ${technology.icon}`);
  const generated = source.replace('<svg ', `<svg fill="${technology.color}" `);
  if (generated === source) throw new Error(`Invalid SVG source for ${technology.icon}`);
  const target = join(output, filename);
  const existing = await readFile(target, 'utf8').catch(() => null);
  if (existing === generated) {
    cached += 1;
  } else {
    await writeFile(target, generated);
    written += 1;
  }
}

let removed = 0;
for (const filename of await readdir(output)) {
  if (!filename.endsWith('.svg') || variants.has(filename)) continue;
  await unlink(join(output, filename));
  removed += 1;
}

console.log(`Technology icons: ${variants.size} total, ${cached} cached, ${written} written, ${removed} removed`);
