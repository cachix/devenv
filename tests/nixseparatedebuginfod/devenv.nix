{ lib
, pkgs
, config
, ...
}:

let
  cfg = config.services.nixseparatedebuginfod;
  fs = lib.fileset;
  inherit (pkgs.stdenv) hostPlatform mkDerivation;

  # Use a locally built derivation so that the test wouldn't rely on cache.nixos.org
  cbin = mkDerivation {
    pname = "cbin";
    version = "1.0";
    src = fs.toSource {
      root = ./.;
      fileset = fs.unions [
        ./example.c
        ./Makefile
      ];
    };
    installFlags = [ "DESTDIR=$(out)" ];
    separateDebugInfo = true;
    meta.mainProgram = "example";
  };
in
lib.mkIf (lib.meta.availableOn hostPlatform cfg.package) {
  services.nixseparatedebuginfod.enable = true;

  enterTest = ''
    wait_for_port ${toString cfg.port} 120
    echo 'Querying ${lib.getExe cbin}'
    ${pkgs.elfutils}/bin/debuginfod-find debuginfo ${lib.getExe cbin}
  '';
}
