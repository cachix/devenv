export interface LandingOption {
  id: string;
  docsId: string | null;
  kind: 'language' | 'service' | 'framework' | 'utility';
  label: string;
  aliases: string[];
  icon: string | null;
  glyph: string;
  color: string;
  darkColor: string;
  option: string | null;
  lines: string[];
  yaml: LandingYamlDocument | null;
}

export type LandingYamlValue = string | number | boolean | null | LandingYamlValue[] | LandingYamlDocument;
export interface LandingYamlDocument { [key: string]: LandingYamlValue }

export interface EnvironmentEntry {
  id: string;
  lineIndexes: number[];
  snippet: string;
}

export interface ParsedEnvironment {
  text: string;
  lines: string[];
  lineEntryIds: string[][];
  entries: EnvironmentEntry[];
}

export interface EntryMeta {
  label: string;
  icon?: string | null;
  glyph?: string;
  color: string;
  darkColor: string;
}

export interface PatienceMatch {
  before: number;
  after: number;
}

export interface DocumentationComment {
  code: string;
  url: string | null;
  label: string | null;
}

const metaColors = ['#6ea8db', '#a98bff', '#62b795', '#e39855', '#d97886', '#6fbcc8'];

function isYamlDocument(value: LandingYamlValue): value is LandingYamlDocument {
  return Boolean(value && typeof value === 'object' && !Array.isArray(value));
}

function mergeYamlValue(current: LandingYamlValue | undefined, incoming: LandingYamlValue): LandingYamlValue {
  if (current === undefined) return structuredClone(incoming);
  if (Array.isArray(current) && Array.isArray(incoming)) {
    const values = current.map((value) => structuredClone(value));
    const serialized = new Set(values.map((value) => JSON.stringify(value)));
    incoming.forEach((value) => {
      const key = JSON.stringify(value);
      if (!serialized.has(key)) {
        serialized.add(key);
        values.push(structuredClone(value));
      }
    });
    return values;
  }
  if (isYamlDocument(current) && isYamlDocument(incoming)) {
    const result: LandingYamlDocument = structuredClone(current);
    Object.entries(incoming).forEach(([key, value]) => {
      result[key] = mergeYamlValue(result[key], value);
    });
    return result;
  }
  return structuredClone(incoming);
}

function yamlScalar(value: string | number | boolean | null) {
  if (value === null) return 'null';
  if (typeof value !== 'string') return String(value);
  if (!value || /^(?:null|true|false|yes|no|on|off|~|[-+]?\d+(?:\.\d+)?)$/i.test(value)) return JSON.stringify(value);
  return /^[A-Za-z0-9_./:+-]+$/.test(value) ? value : JSON.stringify(value);
}

function yamlValueLines(value: LandingYamlValue, indentation: number): string[] {
  const padding = ' '.repeat(indentation);
  if (Array.isArray(value)) {
    return value.flatMap((item) => {
      if (!Array.isArray(item) && !isYamlDocument(item)) return [`${padding}- ${yamlScalar(item)}`];
      const lines = yamlValueLines(item, indentation + 2);
      if (!lines.length) return [`${padding}- {}`];
      return [`${padding}- ${lines[0].trimStart()}`, ...lines.slice(1)];
    });
  }
  if (isYamlDocument(value)) {
    return Object.entries(value).flatMap(([key, item]) => {
      const name = /^[A-Za-z0-9_.-]+$/.test(key) ? key : JSON.stringify(key);
      if (!Array.isArray(item) && !isYamlDocument(item)) return [`${padding}${name}: ${yamlScalar(item)}`];
      if (Array.isArray(item) && item.length === 0) return [`${padding}${name}: []`];
      if (isYamlDocument(item) && Object.keys(item).length === 0) return [`${padding}${name}: {}`];
      const lines = yamlValueLines(item, indentation + 2);
      return [`${padding}${name}:`, ...lines];
    });
  }
  return [`${padding}${yamlScalar(value)}`];
}

export function normalizePrompt(value: string) {
  return value
    .toLowerCase()
    .replace(/c\+\+/g, 'cplusplus')
    .replace(/c#/g, 'csharp')
    .replace(/f#/g, 'fsharp')
    .replace(/\.net/g, 'dotnet')
    .replace(/node\.?js/g, 'nodejs')
    .replace(/next\.?js/g, 'nextjs')
    .replace(/[^a-z0-9]+/g, ' ')
    .trim();
}

export function includesPhrase(prompt: string, phrase: string) {
  const normalizedPhrase = normalizePrompt(phrase);
  return Boolean(normalizedPhrase && ` ${prompt} `.includes(` ${normalizedPhrase} `));
}

export function documentationComment(line: string): DocumentationComment {
  const commentIndex = line.indexOf('#');
  if (commentIndex < 0) return { code: line, url: null, label: null };
  const comment = line.slice(commentIndex).trim();
  const match = comment.match(/https:\/\/[^\s<>"']+/);
  if (!match) return { code: line, url: null, label: null };
  const url = match[0].replace(/[),.;:!?\]}]+$/, '');
  const beforeComment = line.slice(0, commentIndex);
  const code = beforeComment.trim() ? beforeComment.trimEnd() : beforeComment;
  const bareDevenvUrl = comment === `# ${url}` && url.startsWith('https://devenv.sh/');
  return {
    code,
    url,
    label: bareDevenvUrl ? `# ${url.slice('https://'.length)}` : comment,
  };
}

function uniqueAnchors(
  before: string[],
  after: string[],
  beforeStart: number,
  beforeEnd: number,
  afterStart: number,
  afterEnd: number,
) {
  const beforeCounts = new Map<string, { count: number; index: number }>();
  const afterCounts = new Map<string, { count: number; index: number }>();
  for (let index = beforeStart; index < beforeEnd; index += 1) {
    const value = beforeCounts.get(before[index]) ?? { count: 0, index };
    value.count += 1;
    value.index = index;
    beforeCounts.set(before[index], value);
  }
  for (let index = afterStart; index < afterEnd; index += 1) {
    const value = afterCounts.get(after[index]) ?? { count: 0, index };
    value.count += 1;
    value.index = index;
    afterCounts.set(after[index], value);
  }

  const pairs: PatienceMatch[] = [];
  beforeCounts.forEach((value, line) => {
    const match = afterCounts.get(line);
    if (value.count === 1 && match?.count === 1) pairs.push({ before: value.index, after: match.index });
  });
  pairs.sort((left, right) => left.before - right.before);

  const pileEnds: number[] = [];
  const previous: number[] = [];
  pairs.forEach((pair, pairIndex) => {
    let low = 0;
    let high = pileEnds.length;
    while (low < high) {
      const middle = (low + high) >> 1;
      if (pairs[pileEnds[middle]].after < pair.after) low = middle + 1;
      else high = middle;
    }
    previous[pairIndex] = low > 0 ? pileEnds[low - 1] : -1;
    pileEnds[low] = pairIndex;
  });

  const anchors: PatienceMatch[] = [];
  let cursor = pileEnds.length ? pileEnds[pileEnds.length - 1] : -1;
  while (cursor !== -1) {
    anchors.unshift(pairs[cursor]);
    cursor = previous[cursor];
  }
  return anchors;
}

export function patienceMatches(before: string[], after: string[]) {
  const matches: PatienceMatch[] = [];
  function visit(beforeStart: number, beforeEnd: number, afterStart: number, afterEnd: number) {
    while (beforeStart < beforeEnd && afterStart < afterEnd && before[beforeStart] === after[afterStart]) {
      matches.push({ before: beforeStart, after: afterStart });
      beforeStart += 1;
      afterStart += 1;
    }

    const tail: PatienceMatch[] = [];
    while (beforeStart < beforeEnd && afterStart < afterEnd && before[beforeEnd - 1] === after[afterEnd - 1]) {
      beforeEnd -= 1;
      afterEnd -= 1;
      tail.unshift({ before: beforeEnd, after: afterEnd });
    }

    const anchors = uniqueAnchors(before, after, beforeStart, beforeEnd, afterStart, afterEnd);
    let previousBefore = beforeStart;
    let previousAfter = afterStart;
    anchors.forEach((anchor) => {
      visit(previousBefore, anchor.before, previousAfter, anchor.after);
      matches.push(anchor);
      previousBefore = anchor.before + 1;
      previousAfter = anchor.after + 1;
    });
    if (anchors.length) visit(previousBefore, beforeEnd, previousAfter, afterEnd);
    matches.push(...tail);
  }
  visit(0, before.length, 0, after.length);
  return matches;
}

export function cleanEnvironmentLines(lines: string[]) {
  const cleaned: string[] = [];
  lines.forEach((line, index) => {
    if (line.trim()) {
      cleaned.push(line);
      return;
    }
    const previous = cleaned[cleaned.length - 1];
    const next = lines.slice(index + 1).find((candidate) => candidate.trim());
    if (previous?.trim() && next?.trim() !== '}' && previous.trim() !== '{ pkgs, ... }: {' && cleaned[cleaned.length - 1] !== '') {
      cleaned.push('');
    }
  });
  return cleaned;
}

export function createLandingEnvironmentCore(catalog: LandingOption[]) {
  const catalogById = Object.fromEntries(catalog.map((item) => [item.id, item])) as Record<string, LandingOption>;
  const entryMeta = Object.fromEntries(catalog.map((item) => [item.id, {
    label: item.label,
    icon: item.icon,
    glyph: item.glyph,
    color: item.color,
    darkColor: item.darkColor,
  }])) as Record<string, EntryMeta>;

  function documentationUrl(item: LandingOption | undefined) {
    return item?.docsId ? `https://devenv.sh/${item.docsId.replace(/^\/|\/$/g, '')}/` : null;
  }

  function decorateLines(item: LandingOption, lines: string[]) {
    const result = lines.slice();
    const url = documentationUrl(item);
    if (url && result.length) {
      const indentation = result[0].match(/^\s*/)?.[0] ?? '';
      result.unshift(`${indentation}# ${url}`);
    }
    return result;
  }

  function contextualLines(item: LandingOption, selectedIds: string[]) {
    let command: string;
    if (selectedIds.includes('languages.python')) command = item.id === 'tasks."app:test"' ? 'pytest' : 'python -m app';
    else if (selectedIds.includes('languages.javascript') || selectedIds.includes('languages.typescript')) command = item.id === 'tasks."app:test"' ? 'npm test' : 'npm run dev';
    else if (selectedIds.includes('languages.go')) command = item.id === 'tasks."app:test"' ? 'go test ./...' : 'go run .';
    else if (selectedIds.includes('languages.ruby')) command = item.id === 'tasks."app:test"' ? 'bundle exec rails test' : 'bundle exec rails server';
    else if (selectedIds.includes('languages.php')) command = item.id === 'tasks."app:test"' ? 'composer test' : 'php -S localhost:8000';
    else command = item.id === 'tasks."app:test"' ? 'cargo test' : 'cargo run';

    if (item.id === 'tasks."app:test"') return [`  tasks."app:test".exec = "${command}";`];
    if (item.id === 'processes.app') return [`  processes.app.exec = "${command}";`];
    if (item.id === 'processes.api') return ['  processes.api.exec =', `    "secretspec run -- ${command}";`];
    return item.lines;
  }

  function selectPromptMatches(query: string, useFallback = true) {
    const prompt = normalizePrompt(query);
    const selected = new Set<string>();
    catalog.forEach((item) => {
      const terms = [item.label, ...item.aliases];
      if (item.kind === 'language' || item.kind === 'service') terms.push(item.id.split('.').slice(1).join('.'));
      if (terms.some((term) => includesPhrase(prompt, term))) selected.add(item.id);
    });

    const hasKind = (kind: LandingOption['kind']) => [...selected].some((id) => catalogById[id]?.kind === kind);
    if (!hasKind('language') && /\b(machine learning|data science|ai)\b/.test(prompt)) selected.add('languages.python');
    if (!hasKind('language') && /\b(frontend|web app|website|browser)\b/.test(prompt)) selected.add('languages.javascript');
    if (!hasKind('language') && /\b(mobile|ios)\b/.test(prompt)) selected.add('languages.dart');
    if (!hasKind('language') && /\b(api|backend|server|cli|systems)\b/.test(prompt)) selected.add('languages.rust');
    if (!hasKind('service') && /\b(database|sql)\b/.test(prompt)) selected.add('services.postgres');
    if (!hasKind('service') && /\b(search engine|full text search)\b/.test(prompt)) selected.add('services.meilisearch');
    if (/\b(observability|metrics|monitoring)\b/.test(prompt)) {
      selected.add('services.opentelemetry-collector');
      selected.add('services.prometheus');
    }
    if (selected.size === 0 && useFallback) {
      selected.add('languages.rust');
      selected.add('services.postgres');
      selected.add('packages.docker');
    }
    if (selected.has('languages.terraform') && !includesPhrase(prompt, 'terraform cli')) selected.delete('packages.terraform');
    return catalog.filter((item) => selected.has(item.id)).map((item) => item.id);
  }

  function buildEnvironment(selectedIds: string[]) {
    const parts = ['{ pkgs, ... }: {'];
    const packages = selectedIds.filter((id) => id.startsWith('packages.'));
    selectedIds.forEach((id) => {
      if (id.startsWith('packages.')) return;
      const item = catalogById[id];
      if (!item) return;
      parts.push(...decorateLines(item, contextualLines(item, selectedIds)), '');
    });
    if (packages.length) {
      parts.push('  packages = [');
      packages.forEach((id) => {
        const item = catalogById[id];
        parts.push(`    # ${documentationUrl(item)}`);
        parts.push(`    pkgs.${id.slice('packages.'.length)}`);
      });
      parts.push('  ];', '');
    }
    while (parts[parts.length - 1] === '') parts.pop();
    parts.push('}');
    return parts.join('\n');
  }

  function buildEnvironmentYaml(selectedIds: string[]) {
    const document = selectedIds.reduce<LandingYamlDocument>((result, id) => {
      const contribution = catalogById[id]?.yaml;
      return contribution ? mergeYamlValue(result, contribution) as LandingYamlDocument : result;
    }, {});
    const lines = yamlValueLines(document, 0);
    return lines.length ? `${lines.join('\n')}\n` : '';
  }

  const optionTemplates = Object.fromEntries(catalog.map((item) => [item.id, decorateLines(item, item.lines)])) as Record<string, string[]>;

  function normalizedEntryId(raw: string) {
    const quoted = raw.match(/^(tasks|processes|scripts)\.("[^"]+"|[^.]+)/);
    if (quoted) return `${quoted[1]}.${quoted[2]}`;
    const parts = raw.split('.');
    if (parts.length < 2) return raw;
    if (parts[0] === 'git-hooks' && parts[1] === 'hooks' && parts[2]) return parts.slice(0, 3).join('.');
    return parts.slice(0, 2).join('.');
  }

  function canonicalEntryId(id: string) {
    if (catalogById[id]) return id;
    const parts = id.split('.');
    if (parts.length < 2) return id;
    const namespace = parts[0];
    const name = normalizePrompt(parts[1].replace(/^"|"$/g, ''));
    const match = catalog.find((item) => {
      if (item.id.split('.')[0] !== namespace) return false;
      const terms = [item.id.split('.').slice(1).join('.'), item.label, ...item.aliases];
      return terms.some((term) => normalizePrompt(term) === name);
    });
    return match?.id ?? id;
  }

  function parseEnvironment(text: string): ParsedEnvironment {
    const lines = text.replace(/\r/g, '').split('\n');
    const lineEntryIds: string[][] = lines.map(() => []);
    const entriesById = new Map<string, EnvironmentEntry>();
    let current: { id: string; depth: number; indent: number } | null = null;
    let inPackages = false;
    const scopes: { path: string; indent: number }[] = [];

    function addEntry(rawId: string, lineIndex: number) {
      const id = canonicalEntryId(rawId);
      if (!entriesById.has(id)) entriesById.set(id, { id, lineIndexes: [], snippet: '' });
      const entry = entriesById.get(id)!;
      if (!entry.lineIndexes.includes(lineIndex)) entry.lineIndexes.push(lineIndex);
      if (!lineEntryIds[lineIndex].includes(id)) lineEntryIds[lineIndex].push(id);
    }

    lines.forEach((line, lineIndex) => {
      const syntaxLine = line.replace(/\s+#\s+https:\/\/devenv\.sh\/\S+\s*$/, '');
      const trimmed = syntaxLine.trim();
      const indentation = syntaxLine.length - syntaxLine.trimStart().length;
      while (scopes.length && indentation <= scopes[scopes.length - 1].indent) scopes.pop();
      if (/^packages\s*=/.test(trimmed)) inPackages = true;

      if (inPackages) {
        for (const match of syntaxLine.matchAll(/pkgs\.([A-Za-z0-9_+.-]+)/g)) addEntry(`packages.${match[1]}`, lineIndex);
        if (/\];/.test(trimmed)) inPackages = false;
        return;
      }

      if (current && indentation > current.indent) {
        if (trimmed) addEntry(current.id, lineIndex);
        current.depth += (syntaxLine.match(/{/g) ?? []).length - (syntaxLine.match(/}/g) ?? []).length;
        if (current.depth <= 0 && /;\s*$/.test(trimmed)) current = null;
        return;
      }

      const optionMatch = trimmed.match(/^((?:"[^"]+"|[A-Za-z][\w-]*)(?:\.(?:"[^"]+"|[\w+:-]+))*)\s*(?:=|$)/);
      if (optionMatch) {
        const rawPath = optionMatch[1];
        const scope = scopes[scopes.length - 1];
        const fullPath = scope && !rawPath.startsWith(`${scope.path}.`) ? `${scope.path}.${rawPath}` : rawPath;
        const id = normalizedEntryId(fullPath);
        const opens = (syntaxLine.match(/{/g) ?? []).length;
        const closes = (syntaxLine.match(/}/g) ?? []).length;
        const isNamespace = fullPath.split('.').length === 1 && catalog.some((item) => item.id.startsWith(`${fullPath}.`));
        if (!isNamespace) addEntry(id, lineIndex);
        if (opens > closes) scopes.push({ path: fullPath, indent: indentation });
        if (isNamespace) return;
        current = opens > closes || !/;\s*$/.test(trimmed) ? { id, depth: opens - closes, indent: indentation } : null;
        return;
      }

      if (current && trimmed) {
        addEntry(current.id, lineIndex);
        current.depth += (syntaxLine.match(/{/g) ?? []).length - (syntaxLine.match(/}/g) ?? []).length;
        if (current.depth <= 0 && /;\s*$/.test(trimmed)) current = null;
      }
    });

    const entries = [...entriesById.values()];
    entries.forEach((entry) => {
      entry.snippet = entry.lineIndexes.map((index) => lines[index]).join('\n');
    });
    return { text, lines, lineEntryIds, entries };
  }

  function labelFromId(id: string) {
    const value = id.split('.').slice(1).join('.').replace(/^"|"$/g, '');
    return value.split(/[-_:]/).filter(Boolean).map((part) => part.charAt(0).toUpperCase() + part.slice(1)).join(' ');
  }

  function metaForEntry(entry: { id: string; snippet: string }): EntryMeta {
    if (entryMeta[entry.id]) return entryMeta[entry.id];
    if (/secretspec/i.test(entry.snippet)) return { label: 'SecretSpec', icon: '1password', color: '#a98bff', darkColor: '#a98bff' };
    const namespace = entry.id.split('.')[0];
    const glyphs: Record<string, string> = { languages: 'λ', services: '◆', packages: '+', tasks: '✓', processes: '›_', env: '$', scripts: './' };
    const hash = Array.from(entry.id).reduce((total, character) => (total * 31 + character.charCodeAt(0)) >>> 0, 0);
    return {
      label: labelFromId(entry.id),
      glyph: glyphs[namespace] ?? '◇',
      color: metaColors[hash % metaColors.length],
      darkColor: metaColors[hash % metaColors.length],
    };
  }

  function addEnvironmentEntry(text: string, id: string) {
    const environment = parseEnvironment(text);
    if (!optionTemplates[id] || environment.entries.some((entry) => entry.id === id)) return text;
    const lines = environment.lines.slice();
    const closingIndex = lines.findLastIndex((line) => line.trim() === '}');
    if (closingIndex === -1) return text;

    if (id.startsWith('packages.')) {
      const packageName = id.slice('packages.'.length);
      const packageStart = lines.findIndex((line) => /^\s*packages\s*=\s*\[/.test(line));
      if (packageStart !== -1) {
        let packageEnd = packageStart;
        while (packageEnd < lines.length && !/\];/.test(lines[packageEnd])) packageEnd += 1;
        if (packageStart === packageEnd) {
          lines[packageStart] = lines[packageStart].replace(/\s*\];/, ` pkgs.${packageName} ];`);
        } else {
          lines.splice(packageEnd, 0, `    # ${documentationUrl(catalogById[id])}`, `    pkgs.${packageName}`);
        }
      } else {
        lines.splice(closingIndex, 0, '', ...optionTemplates[id]);
      }
    } else {
      lines.splice(closingIndex, 0, '', ...optionTemplates[id]);
    }

    return cleanEnvironmentLines(lines).join('\n');
  }

  function removeEnvironmentEntry(text: string, id: string) {
    const environment = parseEnvironment(text);
    const entry = environment.entries.find((candidate) => candidate.id === id);
    if (!entry) return text;
    let lines = environment.lines.slice();

    if (id.startsWith('packages.')) {
      const escapedName = id.slice('packages.'.length).replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
      const packagePattern = new RegExp(`\\s*pkgs\\.${escapedName}`, 'g');
      const removedPackageLines = new Set<number>();
      entry.lineIndexes.forEach((lineIndex) => {
        if (/pkgs\./.test(lines[lineIndex].replace(packagePattern, ''))) return;
        removedPackageLines.add(lineIndex);
        if (lineIndex > 0 && /^\s*#\s+https:\/\/devenv\.sh\/packages\/\s*$/.test(lines[lineIndex - 1])) {
          removedPackageLines.add(lineIndex - 1);
        }
      });
      lines = lines.map((line, lineIndex) => removedPackageLines.has(lineIndex) ? '' : line.replace(packagePattern, ''));
      const packageStart = lines.findIndex((line) => /^\s*packages\s*=\s*\[/.test(line));
      if (packageStart !== -1) {
        let packageEnd = packageStart;
        while (packageEnd < lines.length && !/\];/.test(lines[packageEnd])) packageEnd += 1;
        if (!lines.slice(packageStart, packageEnd + 1).some((line) => /pkgs\./.test(line))) {
          lines.splice(packageStart, packageEnd - packageStart + 1);
        }
      }
    } else {
      const removedLines = new Set(entry.lineIndexes);
      const firstLine = Math.min(...entry.lineIndexes);
      if (firstLine > 0 && /^\s*#\s+https:\/\/devenv\.sh\/\S+\s*$/.test(lines[firstLine - 1])) {
        removedLines.add(firstLine - 1);
      }
      lines = lines.filter((_line, index) => !removedLines.has(index));
    }

    return cleanEnvironmentLines(lines).join('\n');
  }

  return {
    catalogById,
    optionTemplates,
    selectPromptMatches,
    documentationUrl,
    buildEnvironment,
    buildEnvironmentYaml,
    parseEnvironment,
    metaForEntry,
    addEnvironmentEntry,
    removeEnvironmentEntry,
  };
}
