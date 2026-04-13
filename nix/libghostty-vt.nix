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
    rev = "01825411ab2720e47e6902e9464e805bc6a062a1";
    hash = "sha256-zDOIAbNdKPfNemiz0aJDjOIWamCpb3FsYxnOr9f2ke0=";
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
