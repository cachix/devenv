{ pkgs, config, lib, inputs, ... }:

let
  cfg = config.languages.rust;
  setup = ''
    inputs:
      fenix:
        url: github:nix-community/fenix
        inputs:
          nixpkgs:
            follow: nixpkgs
  '';

  fenix' = inputs.fenix or (throw "to use languages.rust, you must add the following to your devenv.yaml:\n\n${setup}");
  fenix = fenix'.packages.${pkgs.stdenv.system};
in
{
  options.languages.rust = {
    enable = lib.mkEnableOption "tools for Rust development";

    channel = lib.mkOption {
      type = lib.types.str;
      description = ''
        The [Rustup channel](https://rust-lang.github.io/rustup/concepts/channels.html) to install.
      '';
      default = "stable";
      defaultText = lib.literalExpression "fenix.packages.${pkgs.stdenv.system}.stable";
    };

    components = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      description = ''
        List of additional [Rustup components](https://rust-lang.github.io/rustup/concepts/components.html)
        to install. Note that this is additive to the components installed by the `profile` option.
      '';
      default = [ "rust-analyzer" ];
      defaultText = lib.literalExpression ''[ "rust-analyzer" ]'';
    };

    toolchain = lib.mkOption {
      type = lib.types.anything;
      description = ''
        The [fenix toolchain](https://github.com/nix-community/fenix#toolchain) to use.
      '';
      default = fenix.${cfg.channel};
      defaultText = lib.literalExpression "fenix.packages.stable";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      (cfg.toolchain.withComponents cfg.components)
    ] ++ lib.optional pkgs.stdenv.isDarwin pkgs.libiconv;

    # enable compiler tooling by default to expose things like cc
    languages.c.enable = lib.mkDefault true;
  };
}
