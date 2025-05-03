# Using Profiles in devenv

Profiles are an effective way to configure different aspects of your development environment based on specific needs. 

For example, you might have separate profiles for frontend and backend development.

## Defining Profiles

First, define profiles in your `devenv.nix` file:

```nix
{ lib, config, ... }:

{
  options = {
    profile = lib.mkOption {
      type = lib.types.enum [ "backend" "frontend" "full" ];
      default = "full";
      description = "Development profile to use";
    };
  };

  # Use profile value in your config
  config = {
    # Example: Conditionally enable services based on profile
    services.redis.enable = lib.mkIf (config.profile == "backend" || config.profile == "full") true;
    
    # Example: Profile-specific processes
    processes = {
      # Backend processes
      api-server = lib.mkIf (config.profile == "backend" || config.profile == "full") {
        exec = "python api.py";
        process-compose.environment = {
          PORT = "8000";
        };
      };
      
      # Frontend processes
      dev-server = lib.mkIf (config.profile == "frontend" || config.profile == "full") {
        exec = "npm run dev";
        process-compose.environment = {
          PORT = "3000";
        };
      };
    };
  };
}
```

## Using Profiles

### Temporary Profile Selection

You can temporarily use a specific profile by using [ad-hoc developer environments](../ad-hoc-developer-environments.md) on the command line:

```shell-session
$ devenv --option profile:string backend up
```

This will start only backend processes. For frontend development, you can run:

```shell-session
$ devenv --option profile:string frontend up
```

And this will only start the frontend dev server.

### Persistent Profile Selection

For a more permanent selection, create a `devenv.local.nix` file (which should be added to `.gitignore`) with your preferred profile:

```nix
{ pkgs, lib, config, ... }: {
  profile = "frontend";
}
```

This allows each developer to choose their preferred profile without affecting others.

This approach is particularly useful for testing specific configurations or when you need a specialized environment for a particular task.

