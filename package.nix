{ pkgs, inputs }:

let
  python_slim = pkgs.python311.override {
    mimetypesSupport = false;
    x11Support = false;
    stripConfig = true;
    stripIdlelib = true;
    stripTests = true;
    stripTkinter = true;
    enableLTO = false;
    rebuildBytecode = false;
    stripBytecode = true;
    includeSiteCustomize = false;
    enableOptimizations = false;
    bzip2 = null;
    gdbm = null;
    xz = null;
    ncurses = null;
    readline = null;
    sqlite = null;
    tzdata = null;
    self = python_slim;
  };
in
(inputs.poetry2nix.legacyPackages.${pkgs.stdenv.system}.mkPoetryApplication {
  projectDir = ./.;
  python = python_slim;
}).overrideAttrs (old: {
  makeWrapperArgs = [
    "--set DEVENV_NIX ${inputs.nix.packages.${pkgs.stdenv.system}.nix}"
  ];
})
