# Build libghostty-vt shared library from ghostty source.
# The pinned commit must match GHOSTTY_COMMIT in the libghostty-vt-sys build.rs.
{ lib
, stdenv
, fetchFromGitHub
, callPackage
, zig_0_15
, git
}:

let
  ghosttySrc = fetchFromGitHub {
    owner = "ghostty-org";
    repo = "ghostty";
    rev = "bebca84668947bfc92b9a30ed58712e1c34eee1d";
    hash = "sha256-7MPEjIAQD+Z/zdP4h/yslysuVnhCESOPvdvwoLoPVmI=";
  };

  ghosttyDeps = callPackage "${ghosttySrc}/build.zig.zon.nix" { };
in
stdenv.mkDerivation {
  pname = "libghostty-vt";
  version = "0.1.0";
  src = ghosttySrc;

  nativeBuildInputs = [ zig_0_15 git ];

  dontConfigure = true;
  dontInstall = true;

  buildPhase = ''
    export HOME=$TMPDIR
    zig build -Demit-lib-vt --system ${ghosttyDeps} --prefix $out -Dcpu=baseline
  '';

  meta = {
    description = "Ghostty terminal VT library";
    platforms = lib.platforms.linux;
  };
}
