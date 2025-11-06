# Profiles

!!! tip "New in 1.9"

    [Read more about profiles in the v1.9 release post](blog/posts/devenv-v1.9-scaling-nix-projects-using-modules-and-profiles.md)

Profiles allow you to organize different variations of your development environment. You can activate profiles manually using CLI flags or have them activate automatically based on your system environment.

### Basics

Define profiles in your `devenv.nix` file using the `profiles` option:

```nix
{ pkgs, config, ... }: {
  profiles = {
    backend.module = {
      services.postgres.enable = true;
      services.redis.enable = true;
      env.ENVIRONMENT = "backend";
    };

    frontend.module = {
      languages.javascript.enable = true;
      processes.dev-server.exec = "npm run dev";
      env.ENVIRONMENT = "frontend";
    };

    testing.module = { pkgs, ... }: {
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

When multiple profiles are active, devenv wraps every profile module in a deterministic priority. Conflicting options are resolved by those priorities instead of relying on evaluation order.

### Referencing `config` in profiles

Each profile is a submodule that gets recursively merged into the top-level configuration.
When you need to reference other configuration values from within a profile, you must make the profile module a function that receives its own `config` argument.

Consider this example that **won't work** as expected:

```nix
{ config, ... }:
{
  profiles.dev.module = {
    # This references the top-level config, which doesn't yet include
    # the postgres configuration from this profile.
    # PGHOST is an environment variable set by the postgres service.
    env.DB_HOST = config.env.PGHOST;
    services.postgres.enable = true;
  };
}
```

The `config` here refers to the top-level configuration, which doesn't yet know about values set within the profile itself.

**The solution** is to make the profile module a function:

```nix
{ config, ... }:
{
  profiles.dev.module = { config, ... }: {
    # Now config includes both top-level and profile-specific values
    env.DB_HOST = config.env.PGHOST;

    services.postgres.enable = true;
  };
}
```

The inner `config` argument contains the merged result of both the top-level configuration and the profile's own settings.

Use the function form when your profile needs to reference `config` values that are set **within the same profile**.
Profiles that only set static values, or that only read from the top-level configuration, can use the shorthand attribute set form.

### Profile priorities

Profile priorities are assigned automatically so you can reason about overrides:

- **Base configuration** always loads first and has the lowest precedence.
- **Hostname profiles** activate next, followed by **user profiles**.
- **Manual profiles** passed with `--profile` have the highest precedence; if you pass several profiles, the last flag wins.
- **Extends chains** resolve parents before children, so child profiles override their parents without extra `mkForce` calls.

This ordering keeps large profile stacks predictable even when several profiles change the same option.

Here is a simple example where every tier toggles the same option, yet the final value stays deterministic:

```nix
{ config, ... }: {
  myteam.services.database.enable = false;

  profiles = {
    hostname."dev-server".module = {
      myteam.services.database.enable = true;
    };

    user."alice".module = {
      myteam.services.database.enable = false;
    };

    qa.module = {
      myteam.services.database.enable = true;
    };
  };
}
```

When Alice runs on `dev-server`, the hostname profile enables the database, her user profile disables it again, and a manual `devenv --profile qa shell` flips it back on. Conflicts resolve in priority order without any extra override helpers.

## Merging profiles

Profiles can extend other profiles using the `extends` option, allowing you to build hierarchical configurations and reduce duplication:

```nix
{
  name = "myproject";

  packages = [ pkgs.git pkgs.curl ];
  languages.nix.enable = true;

  profiles = {
    backend = {
      module = {
        services.postgres.enable = true;
        services.redis.enable = true;
      };
    };

    frontend = {
      module = {
        languages.javascript.enable = true;
        processes.dev-server.exec = "npm run dev";
      };
    };

    fullstack = {
      extends = [ "backend" "frontend" ];
    };
  };
}
```

## Hostname Profiles

Profiles can automatically activate based on your machine's hostname:

```nix
{
  profiles = {
    work-tools.module = {
      packages = [ pkgs.docker pkgs.kubectl pkgs.slack ];
    };

    hostname = {
      "work-laptop" = {
        extends = [ "work-tools" ];
        module = {
          env.WORK_ENV = "true";
          services.postgres.enable = true;
        };
      };

      "home-desktop".module = {
        env.PERSONAL_DEV = "true";
      };
    };
  };
}
```

## User Profiles

Profiles can automatically activate based on your username:

```nix
{
  profiles = {
    developer-base.module = {
      packages = [ pkgs.git pkgs.gh pkgs.jq ];
      git.enable = true;
    };

    user = {
      "alice" = {
        extends = [ "developer-base" ];
        module = {
          env.USER_ROLE = "backend-developer";
          languages.python.enable = true;
        };
      };

      "bob" = {
        extends = [ "developer-base" ];
        module = {
          env.USER_ROLE = "systems-engineer";
          languages.go.enable = true;
          languages.rust.enable = true;
        };
      };
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
    backend.module = {
      services.postgres.enable = true;
    };

    hostname."ci-server".module = {
      env.CI = "true";
      packages = [ pkgs.buildkit ];
    };

    user."developer".module = {
      git.enable = true;
      packages = [ pkgs.gh ];
    };
  };
}
```

When you run `devenv --profile backend shell` on a machine named "ci-server" with user "developer", all matching profiles activate:

- Base configuration (always active)
- `profiles.backend` (via `--profile`)
- `profiles.hostname."ci-server"` (automatic hostname match)
- `profiles.user."developer"` (automatic user match)
