# { pkgs, config, lib, inputs, ... }:
{ }

# let
#   cfg = config.languages.rust;
#   setup = ''
#     inputs:
#       fenix:
#         url: github:nix-community/fenix
#         inputs:
#           nixpkgs:
#             follows: nixpkgs
#   '';

#   fenix-input = dbg: inputs.fenix or (throw "To use languages.rust.${dbg}, you need to add the following to your devenv.yaml:\n\n${setup}");

#   tryPath = p: pkgs.lib.optional (pkgs.lib.pathExists p) p;
# in
# {
#   options.languages.rust' = {
#     enable = lib.mkEnableOption "tools for Rust development";

#     components = lib.mkOption {
#       type = lib.types.nullOr (lib.types.listOf lib.types.str);
#       default = null;
#       defaultText = lib.literalExpression "null";
#       description = "Rust components to install.";
#     };

#     package = lib.mkOption {
#       type = lib.types.package;
#       defaultText = lib.literalExpression "nixpkgs";
#       default = pkgs.symlinkJoin {
#         name = "nixpkgs-rust";
#         paths = with pkgs; [
#           rustc
#           cargo
#           rustfmt
#           clippy
#           rust-analyzer
#         ];
#         postBuild = ''
#           for bin in $out/bin/*; do
#             if [[ -x $bin ]]; then
#               wrapProgram $bin
#                 --set RUST_SRC_PATH ${pkgs.rustPlatform.rustLibSrc}
#             fi
#           done
#         '';
#       };
#       description = "Rust package including rustc and Cargo.";
#     };

#     profile = lib.mkOption {
#       type = lib.types.nullOr (lib.types.enum [ "minimal" "default" "complete" ]);
#       description = ''
#         The [rustup profile](https://rust-lang.github.io/rustup/concepts/profiles.html)
#       '';
#     };

#     toolchain = lib.mkOption {
#       type = lib.types.nullOr (
#         lib.types.lazyAttrsOf
#           ((fenix-input "toolchain").packages.${pkgs.stdenv.system}.stable).type
#       );
#       description = ''
#         The [fenix toolchain](https://github.com/nix-community/fenix#toolchain) to use.

#         To use fenix, add the following to your devenv.yaml:
#         ```yaml title="devenv.yaml"
#         ${setup}
#         ```
#       '';
#       default = null;
#       defaultText = lib.literalExpression "null";
#     };

#     version = lib.mkOption {
#       type = lib.types.nullOr lib.types.str;
#       default = null;
#       description = "Set to stable, beta, or latest.";
#       defaultText = "null";
#     };
#   };

#   config = lib.mkMerge [
#     (lib.mkIf cfg.enable {
#       packages = [ cfg.package ] ++ lib.optional pkgs.stdenv.isDarwin pkgs.libiconv;

#       # enable compiler tooling by default to expose things like cc
#       languages.c.enable = lib.mkDefault true;

#       pre-commit.tools.cargo = tryPath "${cfg.package}/bin/cargo";
#       pre-commit.tools.rustfmt = tryPath "${cfg.package}/bin/rustfmt";
#       pre-commit.tools.clippy = tryPath "${cfg.package}/bin/clippy";
#     })
#     (lib.mkIf (cfg.enable && pkgs.stdenv.isDarwin) {
#       env.RUSTFLAGS = [ "-L framework=${config.env.DEVENV_PROFILE}/Library/Frameworks" ];
#       env.RUSTDOCFLAGS = [ "-L framework=${config.env.DEVENV_PROFILE}/Library/Frameworks" ];
#       env.CFLAGS = [ "-iframework ${config.env.DEVENV_PROFILE}/Library/Frameworks" ];
#     })
#     (lib.mkIf (cfg.toolchain != null && cfg.version != null)
#       throw "cannot specify both languages.rust.toolchain and languages.rust.version"
#     )
#     (lib.mkIf (cfg.toolchain != null && cfg.components != null) {
#       languages.rust.package = cfg.toolchain.withComponents cfg.components;
#     })
#   ];
# }
