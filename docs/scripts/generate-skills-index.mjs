import { createHash } from 'node:crypto';
import { readdir, readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';

const schema = 'https://schemas.agentskills.io/discovery/0.2.0/schema.json';
const skillsDir = new URL('../public/.well-known/agent-skills/', import.meta.url);

function frontmatterValue(source, key) {
  const match = source.match(/^---\r?\n([\s\S]*?)\r?\n---/);
  if (!match) return null;
  const value = match[1].match(new RegExp(`^${key}:\\s*(.+)$`, 'm'))?.[1]?.trim();
  return value?.replace(/^['"]|['"]$/g, '') ?? null;
}

const skills = [];
for (const entry of (await readdir(skillsDir, { withFileTypes: true }))
  .filter((item) => item.isDirectory())
  .sort((a, b) => a.name.localeCompare(b.name))) {
  const path = new URL(`${entry.name}/SKILL.md`, skillsDir);
  const contents = await readFile(path);
  const source = contents.toString('utf8');
  const name = frontmatterValue(source, 'name');
  const description = frontmatterValue(source, 'description');
  if (!name || !description) continue;

  skills.push({
    name,
    type: 'skill-md',
    description,
    url: `/.well-known/agent-skills/${entry.name}/SKILL.md`,
    digest: `sha256:${createHash('sha256').update(contents).digest('hex')}`,
  });
}

await writeFile(
  join(skillsDir.pathname, 'index.json'),
  `${JSON.stringify({ $schema: schema, skills }, null, 2)}\n`,
);
