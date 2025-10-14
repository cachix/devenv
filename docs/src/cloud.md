# Cloud

!!! note "[cloud.devenv.sh](https://cloud.devenv.sh) is in private beta, sign up for early access"

### Basic Configuration

Create a `devenv.nix` file in your project root:

```nix
{ pkgs, ... }: {
  languages = {
    python.enable = true;
    nodejs.enable = true;
  };
  
  packages = with pkgs; [
    git
    curl
    jq
  ];
  
  services.postgres = {
    enable = true;
    initialDatabases = [{ name = "myapp"; }];
  };
}
```

### Local-first with conditionals on Cloud

Use `config.cloud.enable` to conditionally configure services:

```nix
{ pkgs, lib, config, ... }: {
  services = {
    # Run PostgreSQL only locally
    postgresql.enable = !config.cloud.enable;
    
    # Use cloud Redis only on cloud
    redis.enable = config.cloud.enable;
  };
}
```

### GitHub CI Integration

Access GitHub context in your configuration:

```nix
{ pkgs, lib, config, ... }: 
let
  github = config.cloud.ci.github;
in {
  git-hooks = {
    hooks.rustfmt.enable = true;
    # Run hooks only on changes
    fromRef = github.base_ref or null;
    toRef = github.ref or null;
  };
  
  tasks = {
    # Branch-specific tasks
    "code-review" = lib.mkIf (github.branch == "main") {
      exec = "claude @code-reviewer";
    };
  };
}
```

