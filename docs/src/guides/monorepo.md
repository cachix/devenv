# Monorepo with Shared Configurations

!!! info "New in version 1.10"

This guide shows how to structure a monorepo where multiple services share common configurations.

!!! tip
    [Profiles](../profiles.md) provide another powerful way to organize development environments by allowing different variations to activate automatically based on your hostname, username, or manually via CLI flags. They work particularly well with monorepo structures for managing team-specific or environment-specific configurations.

## Project Structure

```
my-monorepo/
├── shared/
│   └── devenv.nix       # Shared configurations
├── services/
│   ├── api/
│   │   ├── devenv.yaml
│   │   └── devenv.nix
│   └── frontend/
│       ├── devenv.yaml
│       └── devenv.nix
```

## Shared Configuration

Create a shared configuration with common settings:

```nix title="shared/devenv.nix"
{ pkgs, ... }: {
  packages = [
    pkgs.curl
    pkgs.jq
  ];

  services.postgres = {
    enable = true;
    initialDatabases = [
      { name = "myapp"; }
    ];
  };

  git-hooks.hooks = {
    prettier.enable = true;
    nixpkgs-fmt.enable = true;
  };
}
```

## Service Configuration

### API Service

Each service imports the shared configuration using an **absolute import path**. Paths starting with `/` are resolved from the repository root (where `.git` is located), allowing services in different directories to reference shared configurations consistently.

```yaml title="services/api/devenv.yaml"
imports:
  - /shared
```

```nix title="services/api/devenv.nix"
{ pkgs, ... }: {
  # Node.js for the API
  languages.javascript = {
    enable = true;
    package = pkgs.nodejs_20;
  };

  # API-specific environment
  env = {
    API_PORT = "3000";
    SERVICE_NAME = "api";
  };

  # API scripts
  scripts = {
    dev.exec = "npm run dev";
    test.exec = "npm test";
  };
}
```

### Frontend Service

```yaml title="services/frontend/devenv.yaml"
imports:
  - /shared
```

```nix title="services/frontend/devenv.nix"
{ pkgs, ... }: {
  languages.javascript = {
    enable = true;
    package = pkgs.nodejs_20;
  };

  # Frontend scripts
  scripts = {
    dev.exec = "npm run dev";
    build.exec = "npm run build";
  };
}
```

## Referencing the Repository Root

When working in a monorepo, you often need to reference paths relative to the repository root. Use `config.git.root` to get the absolute path to the git repository root.

This is particularly useful for running processes from specific directories:

```nix title="services/api/devenv.nix"
{ pkgs, config, ... }: {
  processes.api.exec = {
    exec = "npm run dev";
    cwd = "${config.git.root}/services/api";
  };

  processes.frontend.exec = {
    exec = "npm run dev";
    cwd = "${config.git.root}/services/frontend";
  };
}
```

This allows you to run multiple service processes from a single devenv shell, regardless of which directory you're in.

## Working with Services

Enter a specific service environment:

```bash
cd services/api
devenv shell
```

The API service will have access to:

- All packages from `shared/devenv.nix` (git, curl, jq)
- The PostgreSQL database service
- Common environment variables (PROJECT_NAME, ENVIRONMENT, DATABASE_URL)
- Its own specific settings (API_PORT, SERVICE_NAME)

Similarly, the frontend service inherits the shared configuration while maintaining its own specific settings.
