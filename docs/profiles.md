# Profiles

!!! info "New in 1.9"

Profiles allow you to organize different variations of your development environment. You can activate profiles manually using CLI flags or have them activate automatically based on your system environment.

### Basics 

Define profiles in your `devenv.nix` file using the `profiles` option:

```nix
{
  profiles = {
    backend.config = {
      services.postgres.enable = true;
      services.redis.enable = true;
      env.ENVIRONMENT = "backend";
    };

    frontend.config = {
      languages.javascript.enable = true;
      processes.dev-server.exec = "npm run dev";
      env.ENVIRONMENT = "frontend";
    };

    testing.config = {
      packages = [ pkgs.playwright pkgs.cypress ];
      env.NODE_ENV = "test";
    };
  };
}
```

Use the `--profile` flag to activate one or more profiles:

```shell-session
# Activate a single profile
$ devenv --profile backend shell

# Activate multiple profiles
$ devenv --profile backend --profile testing shell
```

When using multiple profiles, configurations are merged with later profiles taking precedence for conflicting options.

### Resolving conflicts

Profile configurations can be functions that receive module arguments, allowing access to `lib`, `config`, and other module system features:

```nix
{
  profiles = {
    base.config = { lib, ... }: {
      env.DEBUG = lib.mkDefault "false";  # Low priority
    };

    development.config = { lib, ... }: {
      env.DEBUG = lib.mkForce "true";     # High priority - overrides base
    };
  };
}
```

## Hostname Profiles

Profiles can automatically activate based on your machine's hostname:

```nix
{
  profiles.hostname = {
    "work-laptop".config = {
      env.WORK_ENV = "true";

      packages = [ pkgs.docker pkgs.kubectl ];

      services.postgres.enable = true;
    };

    "home-desktop".config = {
      env.PERSONAL_DEV = "true";
    };
  };
}
```

## User Profiles

Profiles can automatically activate based on your username:

```nix
{
  profiles.user = {
    "alice".config = {
      env.USER_ROLE = "backend-developer";

      languages.python.enable = true;
    };

    "bob".config = {
      env.USER_ROLE = "systems-engineer";

      languages.go.enable = true;
      languages.rust.enable = true;
    };
  };
}
```

## Composition

All matching profiles are automatically merged when you run devenv commands:

```nix
{
  languages.nix.enable = true;
  
  profiles = {
    backend.config = {
      services.postgres.enable = true;
    };
    
    hostname."ci-server".config = {
      env.CI = "true";
      packages = [ pkgs.buildkit ];
    };
    
    user."developer".config = {
      git.enable = true;
      packages = [ pkgs.gh ];
    };
  };
}
```

When you run `devenv --profile backend shell` on a machine named "ci-server" with user "developer", all matching profiles activate:

- Base configuration (always active)
- `profiles.backend` (manual via `--profile`)  
- `profiles.hostname."ci-server"` (automatic hostname match)
- `profiles.user."developer"` (automatic user match)

