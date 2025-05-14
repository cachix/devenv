cat > devenv.local.nix << EOF
{ pkgs, lib, config, ... }: {
  # racket on macOS is broken
  languages.racket.enable = lib.mkForce (
    !(builtins.elem pkgs.stdenv.system config.languages.racket.package.meta.badPlatforms)
  );
}
EOF
