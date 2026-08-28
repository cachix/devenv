export type TechnologyKind = 'language' | 'service' | 'framework' | 'utility';
export type TechnologyYamlValue = string | number | boolean | null | TechnologyYamlValue[] | { [key: string]: TechnologyYamlValue };
export type TechnologyYamlDocument = { [key: string]: TechnologyYamlValue };

export interface TechnologyDefinition {
  id: string;
  docsId: string | null;
  kind: TechnologyKind;
  label: string;
  aliases: string[];
  icon: string | null;
  glyph: string;
  color: string;
  darkColor?: string;
  nix: {
    option: string | null;
    lines: string[];
  };
  yaml?: TechnologyYamlDocument;
}

interface ModuleSeed {
  slug: string;
  label: string;
  icon: string | null;
  glyph?: string;
  aliases?: string[];
  option?: string;
  lines?: string[];
  color?: string;
  darkColor?: string;
  yaml?: TechnologyYamlDocument;
}

type ServiceSeed = ModuleSeed & Required<Pick<ModuleSeed, 'color' | 'darkColor'>>;

const colors = ['#73a7f1', '#9a8ee8', '#64b99a', '#df9a62', '#d97583', '#66b7c5'];

const languageSeeds: ModuleSeed[] = [
  { slug: 'ansible', label: 'Ansible', icon: 'ansible' },
  { slug: 'c', label: 'C', icon: 'c', aliases: ['c language', 'gcc', 'clang'] },
  { slug: 'clojure', label: 'Clojure', icon: 'clojure' },
  { slug: 'cplusplus', label: 'C++', icon: 'cplusplus', aliases: ['cpp'] },
  { slug: 'crystal', label: 'Crystal', icon: 'crystal' },
  { slug: 'cue', label: 'CUE', icon: null },
  { slug: 'dart', label: 'Dart', icon: 'dart', aliases: ['flutter'] },
  { slug: 'deno', label: 'Deno', icon: 'deno' },
  { slug: 'dotnet', label: '.NET', icon: 'dotnet', aliases: ['dotnet', 'c#', 'f#'] },
  { slug: 'elixir', label: 'Elixir', icon: 'elixir', aliases: ['phoenix'] },
  { slug: 'elm', label: 'Elm', icon: 'elm' },
  { slug: 'erlang', label: 'Erlang', icon: 'erlang' },
  { slug: 'fortran', label: 'Fortran', icon: 'fortran' },
  { slug: 'gawk', label: 'GNU Awk', icon: 'gnu', aliases: ['awk'] },
  { slug: 'gleam', label: 'Gleam', icon: 'gleam' },
  { slug: 'go', label: 'Go', icon: 'go', aliases: ['golang'] },
  { slug: 'hare', label: 'Hare', icon: null },
  { slug: 'haskell', label: 'Haskell', icon: 'haskell', aliases: ['ghc'] },
  { slug: 'helm', label: 'Helm', icon: 'helm' },
  { slug: 'idris', label: 'Idris', icon: null },
  { slug: 'java', label: 'Java', icon: 'openjdk', aliases: ['spring', 'maven', 'gradle', 'jvm'] },
  { slug: 'javascript', label: 'JavaScript', icon: 'javascript', aliases: ['node', 'node.js', 'nodejs', 'react', 'vue', 'svelte', 'next.js', 'express', 'npm', 'pnpm', 'yarn', 'frontend'], lines: ['  languages.javascript = {', '    enable = true;', '    package = pkgs.nodejs_22;', '  };'], color: '#F7DF1E', darkColor: '#F7DF1E' },
  { slug: 'jsonnet', label: 'Jsonnet', icon: null },
  { slug: 'julia', label: 'Julia', icon: 'julia' },
  { slug: 'kotlin', label: 'Kotlin', icon: 'kotlin', aliases: ['android'] },
  { slug: 'lean4', label: 'Lean 4', icon: null },
  { slug: 'lobster', label: 'Lobster', icon: null },
  { slug: 'lua', label: 'Lua', icon: 'lua' },
  { slug: 'nim', label: 'Nim', icon: 'nim' },
  { slug: 'nix', label: 'Nix', icon: 'nixos' },
  { slug: 'ocaml', label: 'OCaml', icon: 'ocaml' },
  { slug: 'odin', label: 'Odin', icon: null },
  { slug: 'opentofu', label: 'OpenTofu', icon: 'opentofu' },
  { slug: 'pascal', label: 'Pascal', icon: null },
  { slug: 'perl', label: 'Perl', icon: 'perl' },
  { slug: 'php', label: 'PHP', icon: 'php', aliases: ['laravel', 'symfony', 'composer'] },
  { slug: 'pkl', label: 'Pkl', icon: null },
  { slug: 'purescript', label: 'PureScript', icon: 'purescript' },
  { slug: 'python', label: 'Python', icon: 'python', aliases: ['django', 'flask', 'fastapi', 'pytorch', 'torch', 'uv', 'pip', 'machine learning', 'ml', 'data science'], lines: ['  languages.python = {', '    enable = true;', '    version = "3.12";', '    venv.enable = true;', '  };'] },
  { slug: 'r', label: 'R', icon: 'r' },
  { slug: 'racket', label: 'Racket', icon: 'racket' },
  { slug: 'raku', label: 'Raku', icon: null },
  { slug: 'robotframework', label: 'Robot Framework', icon: 'robotframework' },
  { slug: 'ruby', label: 'Ruby', icon: 'ruby', aliases: ['rails'], lines: ['  languages.ruby = {', '    enable = true;', '    version = "3.3";', '  };'] },
  { slug: 'rust', label: 'Rust', icon: 'rust', aliases: ['cargo', 'rustc'], lines: ['  languages.rust = {', '    enable = true;', '    channel = "stable";', '  };'], color: '#CE422B', darkColor: '#E8664A' },
  { slug: 'scala', label: 'Scala', icon: 'scala' },
  { slug: 'shell', label: 'Shell', icon: 'gnubash', aliases: ['bash', 'zsh', 'fish'] },
  { slug: 'solidity', label: 'Solidity', icon: 'solidity' },
  { slug: 'standardml', label: 'Standard ML', icon: null },
  { slug: 'swift', label: 'Swift', icon: 'swift' },
  { slug: 'terraform', label: 'Terraform', icon: 'terraform' },
  { slug: 'texlive', label: 'TeX Live', icon: 'latex' },
  { slug: 'typescript', label: 'TypeScript', icon: 'typescript', aliases: ['ts'] },
  { slug: 'typst', label: 'Typst', icon: 'typst' },
  { slug: 'unison', label: 'Unison', icon: null },
  { slug: 'v', label: 'V', icon: null },
  { slug: 'vala', label: 'Vala', icon: null },
  { slug: 'zig', label: 'Zig', icon: 'zig' },
];

const serviceSeeds: ServiceSeed[] = [
  { slug: 'adminer', label: 'Adminer', icon: 'adminer', color: '#34567C', darkColor: '#7FA6C9' },
  { slug: 'blackfire', label: 'Blackfire', icon: null, color: '#65319E', darkColor: '#A875D5' },
  { slug: 'caddy', label: 'Caddy', icon: 'caddy', aliases: ['reverse proxy'], color: '#1F88C0', darkColor: '#48A9DB' },
  { slug: 'cassandra', label: 'Cassandra', icon: 'apachecassandra', color: '#1287B1', darkColor: '#44B5D9' },
  { slug: 'clickhouse', label: 'ClickHouse', icon: 'clickhouse', color: '#FFCC01', darkColor: '#FFCC01' },
  { slug: 'cockroachdb', label: 'CockroachDB', icon: 'cockroachlabs', color: '#6933FF', darkColor: '#9674FF' },
  { slug: 'couchdb', label: 'CouchDB', icon: 'apachecouchdb', color: '#E42528', darkColor: '#F05A5C' },
  { slug: 'dynamodb-local', label: 'DynamoDB Local', icon: null, aliases: ['dynamodb'], color: '#C925D1', darkColor: '#E46AE9' },
  { slug: 'elasticmq', label: 'ElasticMQ', icon: null, color: '#86BD48', darkColor: '#A9D66F' },
  { slug: 'elasticsearch', label: 'Elasticsearch', icon: 'elasticsearch', aliases: ['elastic'], color: '#005571', darkColor: '#36B8C5' },
  { slug: 'garage', label: 'Garage', icon: null, color: '#FF9329', darkColor: '#FFAA55' },
  { slug: 'httpbin', label: 'httpbin', icon: null, color: '#151513', darkColor: '#F1F1EC' },
  { slug: 'influxdb', label: 'InfluxDB', icon: 'influxdb', color: '#22ADF6', darkColor: '#22ADF6' },
  { slug: 'kafka', label: 'Kafka', icon: 'apachekafka', color: '#231F20', darkColor: '#F4F1F2' },
  { slug: 'keycloak', label: 'Keycloak', icon: 'keycloak', color: '#4D4D4D', darkColor: '#B8B8B8' },
  { slug: 'mailhog', label: 'MailHog', icon: null, color: '#95252A', darkColor: '#D96C70' },
  { slug: 'mailpit', label: 'Mailpit', icon: null, color: '#00B786', darkColor: '#2FD5A3' },
  { slug: 'meilisearch', label: 'Meilisearch', icon: 'meilisearch', color: '#FF5CAA', darkColor: '#FF7DBA' },
  { slug: 'memcached', label: 'Memcached', icon: null, color: '#2D948B', darkColor: '#63C5BB' },
  { slug: 'minio', label: 'MinIO', icon: 'minio', aliases: ['s3', 'object storage'], color: '#C72E49', darkColor: '#E76078' },
  { slug: 'mongodb', label: 'MongoDB', icon: 'mongodb', aliases: ['mongo', 'document store'], color: '#47A248', darkColor: '#65C466' },
  { slug: 'mosquitto', label: 'Mosquitto', icon: 'eclipsemosquitto', color: '#3C5280', darkColor: '#7F95C5' },
  { slug: 'mysql', label: 'MySQL', icon: 'mysql', color: '#4479A1', darkColor: '#71A8D1' },
  { slug: 'nats', label: 'NATS', icon: 'natsdotio', color: '#27AAE1', darkColor: '#45BDED' },
  { slug: 'nginx', label: 'nginx', icon: 'nginx', color: '#009639', darkColor: '#33C66A' },
  { slug: 'nixseparatedebuginfod', label: 'Nixseparatedebuginfod', icon: 'nixos', color: '#5277C3', darkColor: '#7FA0E2' },
  { slug: 'opensearch', label: 'OpenSearch', icon: 'opensearch', color: '#005EB8', darkColor: '#4A9BE8' },
  { slug: 'opentelemetry-collector', label: 'OpenTelemetry Collector', icon: 'opentelemetry', color: '#000000', darkColor: '#F5F5F5' },
  { slug: 'postgres', label: 'PostgreSQL', icon: 'postgresql', aliases: ['postgres', 'pgsql', 'sql'], color: '#4169E1', darkColor: '#7895F0', lines: ['  services.postgres = {', '    enable = true;', '    initialDatabases = [{ name = "app"; }];', '  };'] },
  { slug: 'prometheus', label: 'Prometheus', icon: 'prometheus', color: '#E6522C', darkColor: '#F07859' },
  { slug: 'rabbitmq', label: 'RabbitMQ', icon: 'rabbitmq', aliases: ['rabbit', 'amqp', 'message queue'], color: '#FF6600', darkColor: '#FF8533' },
  { slug: 'redis', label: 'Redis', icon: 'redis', aliases: ['cache'], color: '#FF4438', darkColor: '#FF6C63' },
  { slug: 'rustfs', label: 'RustFS', icon: 'rustfs', color: '#0196D0', darkColor: '#32BCEB' },
  { slug: 'sqld', label: 'sqld', icon: 'turso', color: '#4FF8D2', darkColor: '#4FF8D2' },
  {
    slug: 'tailscale',
    label: 'Tailscale Funnel',
    aliases: ['tailscale', 'funnel', 'tunnel'],
    icon: 'tailscale',
    color: '#242424',
    darkColor: '#F2F2F2',
    option: 'services.tailscale.funnel.enable',
    lines: [
      '  services.tailscale.funnel = {',
      '    enable = true;',
      '    target = "localhost:3000";',
      '  };',
    ],
  },
  { slug: 'temporal', label: 'Temporal', icon: 'temporal', color: '#000000', darkColor: '#FFFFFF' },
  { slug: 'tideways', label: 'Tideways', icon: null, color: '#34495E', darkColor: '#7E98AD' },
  { slug: 'trafficserver', label: 'Traffic Server', icon: 'apache', color: '#D22128', darkColor: '#EE5C61' },
  { slug: 'typesense', label: 'Typesense', icon: null, color: '#C0FF58', darkColor: '#C0FF58' },
  { slug: 'varnish', label: 'Varnish', icon: null, color: '#1C6BAB', darkColor: '#5CA4DF' },
  { slug: 'vault', label: 'Vault', icon: 'vault', aliases: ['hashicorp vault'], color: '#FFEC6E', darkColor: '#FFEC6E' },
  { slug: 'wiremock', label: 'WireMock', icon: null, color: '#0FB2EF', darkColor: '#0FB2EF' },
];

const utilityRegistry: TechnologyDefinition[] = [
  { id: 'packages.docker', docsId: 'packages', kind: 'utility', label: 'Docker', aliases: ['container', 'containers'], icon: 'docker', glyph: '+', color: '#54b9f5', nix: { option: null, lines: ['  packages = [ pkgs.docker ];'] } },
  { id: 'packages.git', docsId: 'packages', kind: 'utility', label: 'Git', aliases: ['version control', 'vcs'], icon: 'git', glyph: '+', color: '#e47758', nix: { option: null, lines: ['  packages = [ pkgs.git ];'] } },
  { id: 'packages.curl', docsId: 'packages', kind: 'utility', label: 'curl', aliases: ['http client'], icon: 'curl', glyph: '+', color: '#72a7e8', nix: { option: null, lines: ['  packages = [ pkgs.curl ];'] } },
  { id: 'packages.jq', docsId: 'packages', kind: 'utility', label: 'jq', aliases: ['json query'], icon: null, glyph: '{}', color: '#88aa68', nix: { option: null, lines: ['  packages = [ pkgs.jq ];'] } },
  { id: 'packages.ripgrep', docsId: 'packages', kind: 'utility', label: 'ripgrep', aliases: ['rg', 'text search'], icon: null, glyph: 'rg', color: '#d19a66', nix: { option: null, lines: ['  packages = [ pkgs.ripgrep ];'] } },
  { id: 'packages.cargo-watch', docsId: 'packages', kind: 'utility', label: 'cargo-watch', aliases: ['hot reload', 'live reload', 'watch'], icon: 'rust', glyph: '+', color: '#d2a85d', nix: { option: null, lines: ['  packages = [ pkgs.cargo-watch ];'] } },
  { id: 'packages.awscli2', docsId: 'packages', kind: 'utility', label: 'AWS CLI', aliases: ['aws', 'amazon web services'], icon: null, glyph: 'AWS', color: '#e2a04c', nix: { option: null, lines: ['  packages = [ pkgs.awscli2 ];'] } },
  { id: 'packages.terraform', docsId: 'packages', kind: 'utility', label: 'Terraform CLI', aliases: ['terraform cli'], icon: 'terraform', glyph: '+', color: '#9b82e5', nix: { option: null, lines: ['  packages = [ pkgs.terraform ];'] } },
  { id: 'packages.nodejs_22', docsId: 'packages', kind: 'utility', label: 'Node.js 22', aliases: ['node 22 package'], icon: 'nodedotjs', glyph: '+', color: '#70b678', nix: { option: null, lines: ['  packages = [ pkgs.nodejs_22 ];'] } },
  { id: 'packages.shellcheck', docsId: 'packages', kind: 'utility', label: 'ShellCheck', aliases: ['shell lint'], icon: null, glyph: '$✓', color: '#68a89d', nix: { option: null, lines: ['  packages = [ pkgs.shellcheck ];'] } },
  { id: 'packages.just', docsId: 'packages', kind: 'utility', label: 'just', aliases: ['justfile', 'command runner'], icon: 'just', glyph: '$_', color: '#c79a6b', nix: { option: null, lines: ['  packages = [ pkgs.just ];'] } },
  { id: 'tasks."app:test"', docsId: 'tasks', kind: 'utility', label: 'Test task', aliases: ['test', 'tests', 'testing', 'test task', 'automation'], icon: null, glyph: '✓', color: '#d2ad5d', nix: { option: null, lines: ['  tasks."app:test".exec = "cargo test";'] } },
  { id: 'processes.app', docsId: 'processes', kind: 'utility', label: 'App process', aliases: ['process', 'dev server', 'run app'], icon: null, glyph: '›_', color: '#62b795', nix: { option: null, lines: ['  processes.app.exec = "npm run dev";'] } },
  { id: 'processes.api', docsId: 'integrations/secretspec', kind: 'utility', label: 'SecretSpec', aliases: ['secret', 'secrets', '1password', 'dotenv'], icon: '1password', glyph: '◇', color: '#a98bff', nix: { option: null, lines: ['  processes.api.exec =', '    "secretspec run -- cargo run";'] } },
  { id: 'git-hooks.hooks.pre-commit', docsId: 'git-hooks', kind: 'utility', label: 'Pre-commit', aliases: ['precommit', 'git hook', 'git hooks', 'hooks'], icon: 'git', glyph: '✓', color: '#e47758', nix: { option: null, lines: ['  git-hooks.hooks.pre-commit.enable = true;'] } },
];

const fallbackGlyph = (label: string) => label.replace(/[^A-Za-z0-9+#]/g, '').slice(0, 2).toUpperCase() || '·';

const moduleRegistry = (namespace: 'languages' | 'services', kind: 'language' | 'service', seeds: ModuleSeed[]) =>
  seeds.map<TechnologyDefinition>((seed, index) => ({
    id: `${namespace}.${seed.slug}`,
    docsId: `${namespace}/${seed.slug}`,
    kind,
    label: seed.label,
    aliases: seed.aliases ?? [],
    icon: seed.icon,
    glyph: seed.glyph ?? fallbackGlyph(seed.label),
    color: seed.color ?? colors[(index + (kind === 'service' ? 2 : 0)) % colors.length],
    darkColor: seed.darkColor ?? seed.color ?? colors[(index + (kind === 'service' ? 2 : 0)) % colors.length],
    nix: {
      option: seed.option ?? `${namespace}.${seed.slug}.enable`,
      lines: seed.lines ?? [`  ${namespace}.${seed.slug}.enable = true;`],
    },
    yaml: seed.yaml,
  }));

export const technologyRegistry: TechnologyDefinition[] = [
  ...moduleRegistry('languages', 'language', languageSeeds),
  ...moduleRegistry('services', 'service', serviceSeeds),
  ...utilityRegistry,
];

export const technologyById = new Map(technologyRegistry.map((technology) => [technology.id, technology]));
export const technologyByDocsId = new Map(
  technologyRegistry.flatMap((technology) => technology.docsId ? [[technology.docsId, technology] as const] : []),
);

export const landingOptionCatalog = technologyRegistry.map((technology) => ({
  id: technology.id,
  docsId: technology.docsId,
  kind: technology.kind,
  label: technology.label,
  aliases: technology.aliases,
  icon: technology.icon,
  glyph: technology.glyph,
  color: technology.color,
  darkColor: technology.darkColor ?? technology.color,
  option: technology.nix.option,
  lines: technology.nix.lines,
  yaml: technology.yaml ?? null,
}));

export function validateTechnologyRegistry(documentedIds: string[], optionIds: string[]) {
  const registryIds = new Set(technologyRegistry.map((technology) => technology.id));
  const registeredDocsIds = new Set(technologyRegistry.flatMap((technology) =>
    technology.docsId && (technology.kind === 'language' || technology.kind === 'service') ? [technology.docsId] : [],
  ));
  const knownOptions = new Set(optionIds);
  const duplicateIds = technologyRegistry
    .map((technology) => technology.id)
    .filter((id, index, ids) => ids.indexOf(id) !== index);
  const missingDocsEntries = documentedIds.filter((id) => !registeredDocsIds.has(id));
  const staleDocsEntries = [...registeredDocsIds].filter((id) => !documentedIds.includes(id));
  const missingOptions = technologyRegistry
    .filter((technology) => technology.nix.option && !knownOptions.has(technology.nix.option))
    .map((technology) => technology.nix.option);
  const incompleteEntries = technologyRegistry
    .filter((technology) => !technology.id || !technology.label || !technology.color || (technology.kind === 'service' && !technology.darkColor) || (!technology.icon && !technology.glyph) || technology.nix.lines.length === 0)
    .map((technology) => technology.id);

  return {
    valid: registryIds.size === technologyRegistry.length && duplicateIds.length === 0 && missingDocsEntries.length === 0 && staleDocsEntries.length === 0 && missingOptions.length === 0 && incompleteEntries.length === 0,
    duplicateIds,
    missingDocsEntries,
    staleDocsEntries,
    missingOptions,
    incompleteEntries,
  };
}
