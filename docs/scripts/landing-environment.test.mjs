import assert from 'node:assert/strict';
import { access, readFile } from 'node:fs/promises';
import test from 'node:test';

import { landingOptionCatalog, technologyRegistry } from '../src/data/technology-registry.ts';
import {
  cleanEnvironmentLines,
  createLandingEnvironmentCore,
  documentationComment,
  patienceMatches,
} from '../src/lib/landing-environment-core.ts';
import { hasMeaningfulYaml } from '../src/lib/environment-generation.ts';

const core = createLandingEnvironmentCore(landingOptionCatalog);
const documentedOptions = JSON.parse(await readFile(new URL('../src/data/options.json', import.meta.url), 'utf8'));

test('every service has theme colors and every declared icon exists', async () => {
  const services = technologyRegistry.filter((technology) => technology.kind === 'service');
  assert.equal(services.length, 42);
  await Promise.all(services.map(async (service) => {
    assert.match(service.color, /^#[0-9a-f]{6}$/i, `${service.id} color`);
    assert.match(service.darkColor, /^#[0-9a-f]{6}$/i, `${service.id} dark color`);
    if (service.icon) {
      await access(new URL(`../node_modules/simple-icons/icons/${service.icon}.svg`, import.meta.url));
    }
  }));
});

test('Rust and JavaScript use their language colors', () => {
  const technologies = new Map(technologyRegistry.map((technology) => [technology.id, technology]));
  assert.deepEqual(
    [technologies.get('languages.rust')?.color, technologies.get('languages.rust')?.darkColor],
    ['#CE422B', '#E8664A'],
  );
  assert.deepEqual(
    [technologies.get('languages.javascript')?.color, technologies.get('languages.javascript')?.darkColor],
    ['#F7DF1E', '#F7DF1E'],
  );
});

test('every service snippet uses documented options', () => {
  const services = technologyRegistry.filter((technology) => technology.kind === 'service');
  services.forEach((service) => {
    assert.ok(Object.hasOwn(documentedOptions, service.nix.option), service.nix.option);
    if (service.id !== 'services.postgres' && service.id !== 'services.tailscale') {
      assert.deepEqual(service.nix.lines, [`  ${service.nix.option} = true;`], service.id);
    }
  });
  assert.ok(Object.hasOwn(documentedOptions, 'services.postgres.initialDatabases'));
  assert.ok(Object.hasOwn(documentedOptions, 'services.tailscale.funnel.target'));
});

test('prompt matching covers exact aliases and semantic stack requests', () => {
  const selected = core.selectPromptMatches('Rust API with PostgreSQL, Redis, Docker, tests, and secrets', false);
  assert.deepEqual(
    ['languages.rust', 'services.postgres', 'services.redis', 'packages.docker', 'tasks."app:test"', 'processes.api']
      .filter((id) => !selected.includes(id)),
    [],
  );
  assert.deepEqual(core.selectPromptMatches('something unrecognized', false), []);
  assert.deepEqual(core.selectPromptMatches('something unrecognized'), [
    'languages.rust',
    'services.postgres',
    'packages.docker',
  ]);
});

test('languages and services always render inside namespace blocks', () => {
  const language = core.buildEnvironment(['languages.rust']);
  assert.match(language, /  languages = \{\n/);
  assert.match(language, /    rust = \{\n/);
  assert.doesNotMatch(language, /languages\.rust/);

  const service = core.buildEnvironment(['services.redis']);
  assert.match(service, /  services = \{\n/);
  assert.match(service, /    redis\.enable = true;/);
  assert.doesNotMatch(service, /services\.redis/);

  const combined = core.buildEnvironment(['languages.rust', 'languages.javascript', 'services.redis']);
  assert.equal(combined.match(/  languages = \{/g)?.length, 1);
  assert.equal(combined.match(/  services = \{/g)?.length, 1);
  assert.deepEqual(
    core.parseEnvironment(combined).entries.map((entry) => entry.id),
    ['languages.rust', 'languages.javascript', 'services.redis'],
  );
});

test('catalog YAML contributions merge into one project file', () => {
  const catalog = landingOptionCatalog.map((option) => {
    if (option.id === 'languages.rust') {
      return {
        ...option,
        yaml: {
          inputs: {
            'rust-overlay': {
              url: 'github:oxalica/rust-overlay',
              inputs: { nixpkgs: { follows: 'nixpkgs' } },
              overlays: ['default'],
            },
          },
        },
      };
    }
    if (option.id === 'packages.git') return { ...option, yaml: { imports: ['./shared'] } };
    if (option.id === 'packages.curl') return { ...option, yaml: { imports: ['./shared', './networking'] } };
    return option;
  });
  const yamlCore = createLandingEnvironmentCore(catalog);

  assert.equal(yamlCore.buildEnvironmentYaml(['services.postgres']), '');
  assert.equal(yamlCore.buildEnvironmentYaml(['languages.rust', 'packages.git', 'packages.curl']), `inputs:
  rust-overlay:
    url: github:oxalica/rust-overlay
    inputs:
      nixpkgs:
        follows: nixpkgs
    overlays:
      - default
imports:
  - ./shared
  - ./networking
`);
});

test('YAML visibility ignores placeholders but keeps commented configurations', () => {
  assert.equal(hasMeaningfulYaml('# See https://devenv.sh/reference/yaml-options/\n'), false);
  assert.equal(hasMeaningfulYaml('# See https://devenv.sh/reference/yaml-options/\ninputs:\n  nixpkgs:\n    url: github:NixOS/nixpkgs\n'), true);
});

test('every catalog entry builds, parses, and links to its documentation', () => {
  assert.equal(landingOptionCatalog.length, 115);
  landingOptionCatalog.forEach((option) => {
    const text = core.buildEnvironment([option.id]);
    const parsed = core.parseEnvironment(text);
    assert.ok(parsed.entries.some((entry) => entry.id === option.id), option.id);
    const documentationIndex = parsed.lines.findIndex((line) => line.includes(`https://devenv.sh/${option.docsId}/`));
    const declarationIndex = parsed.lines.findIndex((line) => parsed.lineEntryIds[parsed.lines.indexOf(line)].includes(option.id));
    assert.ok(documentationIndex >= 0, `${option.id} documentation`);
    assert.equal(documentationIndex + 1, declarationIndex, `${option.id} comment placement`);
  });
});

test('the complete catalog round-trips through one rich environment', () => {
  const ids = landingOptionCatalog.map((option) => option.id);
  const text = core.buildEnvironment(ids);
  const parsedIds = new Set(core.parseEnvironment(text).entries.map((entry) => entry.id));
  assert.deepEqual(ids.filter((id) => !parsedIds.has(id)), []);
  assert.equal(parsedIds.size, ids.length);
});

test('every entry can be added and removed without collapsing section spacing', () => {
  let text = core.buildEnvironment([]);
  landingOptionCatalog.forEach((option) => {
    text = core.addEnvironmentEntry(text, option.id);
    assert.ok(core.parseEnvironment(text).entries.some((entry) => entry.id === option.id), `add ${option.id}`);
    assert.doesNotMatch(text, /\n{3,}/, `add spacing ${option.id}`);
  });
  assert.equal(core.parseEnvironment(text).entries.length, landingOptionCatalog.length);

  landingOptionCatalog.forEach((option) => {
    text = core.removeEnvironmentEntry(text, option.id);
    assert.ok(!core.parseEnvironment(text).entries.some((entry) => entry.id === option.id), `remove ${option.id}`);
    assert.doesNotMatch(text, /\n{3,}/, `remove spacing ${option.id}`);
  });
  assert.equal(text, '{ pkgs, ... }: {\n}');
});

test('AI-style nested declarations map to known and inferred ingredients', () => {
  const parsed = core.parseEnvironment(`{ pkgs, ... }: {
  languages.python = {
    enable = true;
    venv.enable = true;
  };

  services.postgres = {
    enable = true;
    initialDatabases = [{ name = "app"; }];
  };

  processes.worker.exec = "secretspec run -- python worker.py";
  packages = [ pkgs.git pkgs.docker ];
}`);
  const ids = new Set(parsed.entries.map((entry) => entry.id));
  assert.deepEqual(
    ['languages.python', 'services.postgres', 'processes.worker', 'packages.git', 'packages.docker']
      .filter((id) => !ids.has(id)),
    [],
  );
  assert.equal(core.metaForEntry(parsed.entries.find((entry) => entry.id === 'processes.worker')).label, 'SecretSpec');
});

test('patience diff keeps stable anchors around inserted and duplicate lines', () => {
  const before = ['a', 'shared', 'x', 'shared', 'z'];
  const after = ['a', 'new', 'shared', 'x', 'shared', 'z'];
  assert.deepEqual(patienceMatches(before, after), [
    { before: 0, after: 0 },
    { before: 1, after: 2 },
    { before: 2, after: 3 },
    { before: 3, after: 4 },
    { before: 4, after: 5 },
  ]);
});

test('documentation parsing and blank-line cleanup preserve readable code', () => {
  assert.deepEqual(documentationComment('  languages.rust.enable = true;  # https://devenv.sh/languages/rust/'), {
    code: '  languages.rust.enable = true;',
    url: 'https://devenv.sh/languages/rust/',
    label: '# devenv.sh/languages/rust/',
  });
  assert.deepEqual(documentationComment('  # See full reference at https://devenv.sh/reference/options/'), {
    code: '  ',
    url: 'https://devenv.sh/reference/options/',
    label: '# See full reference at https://devenv.sh/reference/options/',
  });
  assert.deepEqual(documentationComment('# Generated from https://example.com/environment.yaml.'), {
    code: '',
    url: 'https://example.com/environment.yaml',
    label: '# Generated from https://example.com/environment.yaml.',
  });
  assert.deepEqual(cleanEnvironmentLines([
    '{ pkgs, ... }: {',
    '',
    '',
    '  languages.rust.enable = true;',
    '',
    '',
    '  services.redis.enable = true;',
    '',
    '}',
  ]), [
    '{ pkgs, ... }: {',
    '  languages.rust.enable = true;',
    '',
    '  services.redis.enable = true;',
    '}',
  ]);
});
