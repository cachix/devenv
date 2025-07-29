# Monorepo with Shared Configurations

!!! info "New in version 1.9"

This guide shows how to structure a monorepo where multiple services share common configurations. 
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

Create a `shared/devenv.nix` with common settings:

```nix
{ pkgs, ... }: {
  # Common environment variables
  env = {
    PROJECT_NAME = "my-monorepo";
    ENVIRONMENT = "development";
  };

  # Common packages
  packages = [
    pkgs.git
    pkgs.curl
    pkgs.jq
  ];

  # Shared database
  services.postgres = {
    enable = true;
    initialDatabases = [
      { name = "myapp"; }
    ];
  };

  env.DATABASE_URL = "postgresql://localhost/myapp";

  # Common git hooks
  pre-commit.hooks = {
    prettier.enable = true;
    nixpkgs-fmt.enable = true;
  };
}
```

## Service Configuration

### API Service

`services/api/devenv.yaml`:

```yaml
imports:
  - /shared
```

`services/api/devenv.nix`:

```nix
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

`services/frontend/devenv.yaml`:

```yaml
imports:
  - /shared
```

`services/frontend/devenv.nix`:

```nix
{ pkgs, ... }: {
  # Node.js for the frontend
  languages.javascript = {
    enable = true;
    package = pkgs.nodejs_20;
  };

  # Frontend-specific environment
  env = {
    FRONTEND_PORT = "3001";
    API_URL = "http://localhost:3000";
    SERVICE_NAME = "frontend";
  };

  # Frontend scripts
  scripts = {
    dev.exec = "npm run dev";
    build.exec = "npm run build";
  };
}
```

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

