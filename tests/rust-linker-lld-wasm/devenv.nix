{ pkgs, lib, ... }:
let
  isLinux = pkgs.stdenv.hostPlatform.isLinux;
  hostLinkerVar = "CARGO_TARGET_${pkgs.stdenv.hostPlatform.rust.cargoEnvVarTarget}_LINKER";
in
{
  # lld.enable must only affect the host target: wasm32 links with rust-lld
  # directly, which rejects C compiler driver flags such as `-fuse-ld=lld`.
  languages.rust = {
    enable = true;
    channel = "stable";
    targets = [ "wasm32-unknown-unknown" ];
    lld.enable = true;
  };

  enterTest = ''
    set -euo pipefail

    # The linker must not be configured through RUSTFLAGS/RUSTDOCFLAGS: those
    # reach every target and make cargo ignore `[build] rustflags`.
    for var in RUSTFLAGS RUSTDOCFLAGS; do
      if [ -n "$(printenv "$var" || true)" ]; then
        echo "$var is set, but it should not be: $(printenv "$var")"
        exit 1
      fi
    done

    linker=$(printenv ${hostLinkerVar} || true)
    if [ -z "$linker" ]; then
      echo "${hostLinkerVar} is not set"
      exit 1
    fi

    workdir=$(mktemp -d)
    cargo new --lib --name lld_check "$workdir/lld-check" >/dev/null
    cd "$workdir/lld-check"

    cat >> Cargo.toml <<'EOF'

    [lib]
    crate-type = ["cdylib", "rlib"]
    EOF

    mkdir -p .cargo
    printf '[build]\nrustflags = ["--cfg", "devenv_custom_cfg"]\n' > .cargo/config.toml

    cat > src/lib.rs <<'EOF'
    /// ```
    /// assert_eq!(lld_check::add(1, 2), 3);
    /// ```
    #[unsafe(no_mangle)]
    pub extern "C" fn add(a: i32, b: i32) -> i32 {
        a + b
    }
    EOF

    # Host build: uses the configured linker and keeps `[build] rustflags`.
    cargo build -vv >host.log 2>&1 || { cat host.log; exit 1; }
    if ! grep -qF -- "-C linker=$linker" host.log; then
      echo "cargo did not pass the host linker to rustc"
      cat host.log
      exit 1
    fi
    if ! grep -qF -- "--cfg devenv_custom_cfg" host.log; then
      echo "the project's [build] rustflags were ignored for the host build"
      cat host.log
      exit 1
    fi

    ${lib.optionalString isLinux ''
      if ! ${pkgs.llvmPackages.bintools}/bin/readelf -p .comment target/debug/liblld_check.so | grep -q "Linker: LLD"; then
        echo "the host library was not linked with LLD"
        ${pkgs.llvmPackages.bintools}/bin/readelf -p .comment target/debug/liblld_check.so
        exit 1
      fi
    ''}

    # Cross build to wasm32: must not receive host linker flags.
    cargo build -vv --target wasm32-unknown-unknown >wasm.log 2>&1 || { cat wasm.log; exit 1; }
    test -f target/wasm32-unknown-unknown/debug/lld_check.wasm
    if ! grep -qF -- "--cfg devenv_custom_cfg" wasm.log; then
      echo "the project's [build] rustflags were ignored for the wasm build"
      cat wasm.log
      exit 1
    fi

    # Doctests are linked by rustdoc, which must also pick up the host linker.
    cargo test --doc
  '';
}
