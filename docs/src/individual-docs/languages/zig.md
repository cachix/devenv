## Getting Started

The simplest way to get started with Zig is to use the `version` attribute, which automatically sets up both the Zig compiler and ZLS (Zig Language Server) from the [zig-overlay](https://github.com/mitchellh/zig-overlay):

```nix
languages.zig = {
  enable = true;
  version = "0.15.1";
};
```

This will automatically:
- Use the specified Zig version from zig-overlay
- Install the corresponding ZLS version (e.g., version "0.15.1" uses ZLS 0.15.0)

Alternatively, you can manually specify packages:

```nix
languages.zig = {
  enable = true;
  package = pkgs.zig;
  zls.package = pkgs.zls;
};
```

[comment]: # (Please add your documentation on top of this line)

@AUTOGEN_OPTIONS@
