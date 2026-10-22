{ system ? builtins.currentSystem }:
let
  pkgs = import (builtins.fetchTree (builtins.fromJSON (builtins.readFile ./nixpkgs.json))).outPath { inherit system; };
  inherit (pkgs) lib;
  evaluate = clang: mask: custom:
    (lib.evalModules {
      specialArgs = { inherit pkgs; };
      modules = [
        ../../src/modules/languages/rust.nix
        ({ lib, ... }: {
          options = {
            packages = lib.mkOption { type = lib.types.listOf lib.types.package; default = []; };
            env = lib.mkOption { type = lib.types.attrsOf lib.types.anything; default = {}; };
            languages.c.enable = lib.mkOption { type = lib.types.bool; };
          };
          config = {
            _module.freeformType = lib.types.lazyAttrsOf lib.types.raw;
            languages.rust = {
              enable = true;
              clangLinker.enable = clang;
              mold.enable = builtins.bitAnd mask 1 != 0;
              lld.enable = builtins.bitAnd mask 2 != 0;
              wild.enable = builtins.bitAnd mask 4 != 0;
              rustflags = lib.optionalString (builtins.bitAnd custom 1 != 0) "--cfg matrix_rust";
              rustdocflags = lib.optionalString (builtins.bitAnd custom 2 != 0) "--cfg matrix_doc";
              lsp.enable = false;
            };
            lib.mkOverrideDefault = lib.mkDefault;
            devenv.state = "/tmp/matrix-state";
          };
        })
      ];
    }).config;
  cases = lib.concatMap (clang: lib.concatMap (mask: map (custom:
    let cfg = evaluate clang mask custom; in {
      name = "${system}-clang-${lib.boolToString clang}-linker-${toString mask}-flags-${toString custom}";
      inherit clang mask custom cfg;
      failures = map (a: a.message) (builtins.filter (a: !a.assertion) cfg.assertions);
    }
  ) (lib.range 0 3)) (lib.range 0 7)) [ false true ];
  valid = builtins.filter (c: c.failures == []) cases;
  build = c: pkgs.runCommand c.name {
    nativeBuildInputs = c.cfg.packages ++ [ pkgs.stdenv.cc ];
  } ''
    mkdir project
    cd project
    export HOME=$TMPDIR/home
    mkdir -p "$HOME" .cargo src
    unset RUSTFLAGS RUSTDOCFLAGS CARGO_ENCODED_RUSTFLAGS CARGO_ENCODED_RUSTDOCFLAGS
    ${lib.concatStringsSep "\n" (lib.mapAttrsToList (k: v: lib.optionalString (v != null) "export ${k}=${lib.escapeShellArg (toString v)}") c.cfg.env)}
    cat > Cargo.toml <<'TOML'
    [package]
    name = "matrix"
    version = "0.1.0"
    edition = "2021"
    TOML
    cat > .cargo/config.toml <<'TOML'
    [build]
    rustflags = ["--cfg", "matrix_project"]
    TOML
    cat > src/lib.rs <<'RS'
    ${lib.optionalString (c.mask == 0 && builtins.bitAnd c.custom 1 == 0) ''
    #[cfg(all(not(doc), not(matrix_project)))]
    compile_error!("project rustflags lost");
    ''}
    ${lib.optionalString (builtins.bitAnd c.custom 1 != 0) ''
    #[cfg(all(not(doc), not(matrix_rust)))]
    compile_error!("custom rustflags lost");
    ''}
    ${lib.optionalString (builtins.bitAnd c.custom 2 != 0) ''
    #[cfg(all(doc, not(matrix_doc)))]
    compile_error!("custom rustdocflags lost");
    ''}
    /// Example:
    /// ```
    /// assert_eq!(matrix::answer(), 42);
    /// ```
    pub fn answer() -> u32 { 42 }
    RS
    echo 'fn main() { assert_eq!(matrix::answer(), 42); }' > src/main.rs
    cargo build --offline -vv > build.log 2>&1 || { cat build.log; exit 1; }
    ./target/debug/matrix
    otool -l ./target/debug/matrix > linker.log
    cargo doc --offline --no-deps > doc.log 2>&1 || { cat doc.log; exit 1; }
    cargo test --offline --doc > doctest.log 2>&1 || { cat doctest.log; exit 1; }
    mkdir -p "$out"
    cp build.log linker.log doc.log doctest.log "$out/"
  '';
in {
  evaluation = map (c: { inherit (c) name failures; packagesEvaluate = if c.failures == [] then (builtins.tryEval (builtins.deepSeq (map (p: p.drvPath) c.cfg.packages) true)).success else null; env = if c.failures == [] then c.cfg.env else null; }) cases;
  builds = map build valid;
  lldBuilds = map build (builtins.filter (c: c.mask == 2) valid);
  identities = map build (builtins.filter (c: c.custom == 0) valid);
}
