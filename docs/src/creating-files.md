# Creating Files

The `files` option allows you to declaratively create configuration files and scripts in your development environment. Files are automatically generated from structured data and symlinked into your project when entering the shell.

This is particularly useful for:

- Generating configuration files from Nix data structures
- Creating executable scripts
- Setting up project-specific configurations
- Ensuring consistent file contents across the team

## Supported Formats

devenv supports multiple file formats out of the box:

- **json** - JSON format
- **ini** - INI format
- **yaml** - YAML format
- **toml** - TOML format
- **text** - Plain text

## Basic Examples

### JSON Files

```nix title="devenv.nix"
{
  files."config.json".json = {
    database = {
      host = "localhost";
      port = 5432;
    };
    features = [ "auth" "api" "ui" ];
  };
}
```

This creates a `config.json` file:
```json
{
  "database": {
    "host": "localhost",
    "port": 5432
  },
  "features": ["auth", "api", "ui"]
}
```

### YAML Files

```nix title="devenv.nix"
{
  files."docker-compose.yml".yaml = {
    version = "3.8";
    services = {
      web = {
        image = "nginx:latest";
        ports = [ "8080:80" ];
      };
    };
  };
}
```

### TOML Files

```nix title="devenv.nix"
{
  files."config.toml".toml = {
    title = "My App Config";

    server = {
      host = "0.0.0.0";
      port = 8000;
    };
  };
}
```

### INI Files

```nix title="devenv.nix"
{
  files."settings.ini".ini = {
    general = {
      debug = "true";
      log_level = "info";
    };
    database = {
      connection_string = "postgres://localhost/mydb";
    };
  };
}
```

### Text Files

For plain text files, simply provide a string:

```nix title="devenv.nix"
{
  files."README.txt".text = ''
    This is a development environment.
    Run `devenv shell` to get started.
  '';
}
```

## Executable Files

You can make any file executable by setting the `executable` attribute:

```nix title="devenv.nix"
{
  files."setup.sh" = {
    text = ''
      #!/bin/bash
      echo "Running setup..."
      npm install
    '';
    executable = true;
  };
}
```

This is particularly useful for:

- Shell scripts that need to be executed
- Custom tooling and utilities
- Git hooks

## Copying Files Instead of Symlinking

By default, files are symlinked to a read-only path in the Nix store, so they cannot be edited.
Set the `copy` attribute to materialize an editable file instead:

```nix title="devenv.nix"
{
  # Seed an editable template once; never overwrites the user's edits.
  files.".env.local".copy = "copy";
  files.".env.local".text = ''
    API_URL=http://localhost:8080
  '';

  # Always overwrite with a fresh writable copy on every shell entry.
  files."config/generated.toml".copy = "replace";
  files."config/generated.toml".toml = {
    server.port = 8000;
  };
}
```

The `copy` attribute accepts:

- `symlink` (default): symlink to the read-only file in the Nix store. Edits are not possible.
- `copy`: copy the file into place once, only if it does not already exist, and make it writable. Existing files are left untouched. This is useful for seeding configuration files from templates that the user can then edit to fit their project.
- `replace`: overwrite the file with a fresh writable copy on every shell entry.

!!! tip "New in version 2.2"

## Creating Files in Subdirectories

Files can be created in nested directories by specifying the path:

```nix title="devenv.nix"
{
  files = {
    ".config/app/settings.json".json = {
      theme = "dark";
    };

    "scripts/build.sh" = {
      text = "#!/bin/bash\nnpm run build";
      executable = true;
    };
  };
}
```

The parent directories will be created automatically if they don't exist.
