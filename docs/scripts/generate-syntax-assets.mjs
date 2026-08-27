import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const source = new URL('../node_modules/shiki/dist/onig.wasm', import.meta.url);
const destination = new URL('../public/shiki/onig.wasm', import.meta.url);
const contents = await readFile(source);
let existing = null;
try {
  existing = await readFile(destination);
} catch {}

if (existing?.equals(contents)) {
  console.log(`Syntax runtime: cached ${contents.byteLength} bytes`);
} else {
  await mkdir(dirname(fileURLToPath(destination)), { recursive: true });
  await writeFile(destination, contents);
  console.log(`Syntax runtime: wrote ${contents.byteLength} bytes`);
}
