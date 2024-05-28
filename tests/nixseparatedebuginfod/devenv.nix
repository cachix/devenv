{ lib, pkgs, config, ... }:

let
  cfg = config.services.nixseparatedebuginfod;
  inherit (pkgs.stdenv) hostPlatform;

  # Use a locally built derivation so that the test wouldn't rely on cache.nixos.org
  cbin = pkgs.stdenv.mkDerivation {
    pname = "cbin";
    version = "1.0";
    src = ./.;
    installFlags = [ "DESTDIR=$(out)" ];
    separateDebugInfo = true;
    meta.mainProgram = "example";
  };
in
lib.mkIf (lib.meta.availableOn hostPlatform cfg.package) {
  services.nixseparatedebuginfod.enable = true;

  # The Nix store needs to be indexed by nixseparatedebuginfod for debug outputs from local
  # derivations to be served. This can take a few minutes.
  process.before = "${lib.getExe cfg.package} --index-only";

  enterTest = ''
    wait_for_port ${toString cfg.port} 120
    ${pkgs.elfutils}/bin/debuginfod-find debuginfo ${lib.getExe cbin}
  '';
}
