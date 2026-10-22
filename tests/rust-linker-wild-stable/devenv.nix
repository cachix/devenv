{ pkgs, ... }:
{
  # rust-overlay toolchains link with rust-lld by default on x86_64 Linux.
  # The selected linker must still win over rustc's own `-fuse-ld=lld`.
  languages.rust = {
    enable = true;
    channel = "stable";
    wild.enable = true;
  };

  enterTest = ''
    set -euo pipefail

    workdir=$(mktemp -d)
    cargo new --bin --name wild_check "$workdir/wild-check" >/dev/null
    cd "$workdir/wild-check"
    cargo build

    comment=$(${pkgs.binutils-unwrapped}/bin/readelf -p .comment target/debug/wild_check)
    if ! grep -q "Linker: Wild" <<<"$comment"; then
      echo "the binary was not linked with wild"
      echo "$comment"
      exit 1
    fi
  '';
}
