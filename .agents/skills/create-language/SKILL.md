---
name: create-language
description: This skill should be used when the user asks to "create a language module", "add a new language", "write a language module for", "implement language support for", or wants to add a new language to src/modules/languages/. Provides the patterns and conventions for devenv language modules.
argument-hint: [language-name]
---

# Create a devenv Language Module

This skill guides the creation of new language modules under `src/modules/languages/`.

## Process

1. Research the language: package name in nixpkgs, LSP server package, common development tools, environment variables, version overlay availability
2. Read existing modules in `src/modules/languages/` for reference (e.g., `nim.nix` for simple, `go.nix` for medium, `rust.nix` for complex)
3. Create `src/modules/languages/<name>.nix` following the patterns below (auto-discovered)
4. Add a test under `tests/`

## Module Structure

Every language module follows this skeleton:

```nix
{ pkgs, config, lib, ... }:

let
  cfg = config.languages.<name>;
in
{
  options.languages.<name> = {
    enable = lib.mkEnableOption "tools for <Name> development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.<name>;
      defaultText = lib.literalExpression "pkgs.<name>";
      description = "The <Name> package to use.";
    };

    # Optional: version pinning via overlay (see Version Pinning below)

    lsp = {
      enable = lib.mkEnableOption "<Name> Language Server" // { default = true; };
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.<lsp-package>;
        defaultText = lib.literalExpression "pkgs.<lsp-package>";
        description = "The <Name> language server package to use.";
      };
    };

    # Add language-specific options here
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ] ++ lib.optional cfg.lsp.enable cfg.lsp.package;

    # Optional: environment variables
    # env.GOROOT = cfg.package + "/share/go/";

    # Optional: PATH additions or shell setup
    # enterShell = ''
    #   export PATH=$SOME_PATH/bin:$PATH
    # '';
  };
}
```

## Key Conventions

### LSP Support

Most language modules include an LSP sub-option. The `enable` default should be `true` so users get IDE support out of the box:

```nix
lsp = {
  enable = lib.mkEnableOption "<Name> Language Server" // { default = true; };
  package = lib.mkOption {
    type = lib.types.package;
    default = pkgs.<lsp-package>;
    defaultText = lib.literalExpression "pkgs.<lsp-package>";
    description = "The <Name> language server package to use.";
  };
};
```

Add the LSP package conditionally:

```nix
packages = [
  cfg.package
] ++ lib.optional cfg.lsp.enable cfg.lsp.package;
```

### Version Pinning via Overlays

When a language has a Nix overlay for version management, use `config.lib.getInput` for lazy input fetching:

```nix
let
  overlay = config.lib.getInput {
    name = "<name>-overlay";
    url = "github:<owner>/<overlay-repo>";
    attribute = "languages.<name>.version";
    follows = [ "nixpkgs" ];
  };
in
{
  options.languages.<name> = {
    version = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = ''
        The <Name> version to use.
        This automatically sets `languages.<name>.package` using [<overlay-name>](<overlay-url>).
      '';
      example = "<example-version>";
    };
    # ...
  };

  config = lib.mkIf cfg.enable {
    languages.<name>.package = lib.mkIf (cfg.version != null) (
      overlay.packages.${pkgs.stdenv.system}.${cfg.version}
    );
    # ...
  };
}
```

The `attribute` field in `getInput` tells devenv which user-facing option triggers fetching this input. The input is only fetched when that option is set to a non-default value.

### Environment Variables

Set language-specific environment variables in `env`:

```nix
env.GOROOT = cfg.package + "/share/go/";
env.GOPATH = config.env.DEVENV_STATE + "/go";
```

Use `config.env.DEVENV_STATE` for persistent state directories (e.g., package caches, installed binaries).

### Shell Setup (enterShell)

Use `enterShell` for PATH additions or runtime setup that can't be done via `env`:

```nix
enterShell = ''
  export PATH=$GOPATH/bin:$PATH
'';
```

### Git Hooks Integration

When the language has formatter/linter tools supported by git-hooks, wire them up:

```nix
# Point hook tools at the configured package
git-hooks.tools = {
  cargo = config.lib.mkOverrideDefault cfg.toolchainPackage;
  rustfmt = config.lib.mkOverrideDefault cfg.toolchainPackage;
};

# Or set hook packages directly
git-hooks.hooks = {
  mix-format.package = cfg.package;
};
```

### Enabling Companion Languages

When a language requires another (e.g., Rust needs a C compiler):

```nix
languages.c.enable = lib.mkDefault true;
```

Use `lib.mkDefault` so users can override if needed.

### Backward Compatibility (Renamed Options)

When migrating options, use `mkRenamedOptionModule` in imports:

```nix
imports = [
  (lib.mkRenamedOptionModule [ "languages" "<name>" "old-option" ] [ "languages" "<name>" "new-option" ])
];
```

### Complexity Levels

**Simple** (nim.nix, elixir.nix): Just `enable`, `package`, `lsp`, and packages list. Use this for languages with straightforward nixpkgs support and no special setup.

**Medium** (go.nix, zig.nix): Adds `version` option with overlay, environment variables, and `enterShell`. Use when the language benefits from version pinning or needs runtime paths configured.

**Complex** (rust.nix, javascript.nix): Multiple sub-features (toolchain management, package managers, build tools), assertions, `mkMerge` for conditional config blocks. Use only when the language ecosystem genuinely demands it.

Start simple and add complexity only as needed.
