{ pkgs, ... }:
{
  # rust-overlay toolchains link with rust-lld by default on x86_64 Linux.
  # The selected linker must still win over rustc's own `-fuse-ld=lld`.
  languages.rust = {
    enable = true;
    channel = "stable";
    mold.enable = true;
  };

  enterTest = ''
    set -euo pipefail

    workdir=$(mktemp -d)
    cargo new --bin --name mold_check "$workdir/mold-check" >/dev/null
    cd "$workdir/mold-check"
    cargo build

    comment=$(${pkgs.binutils-unwrapped}/bin/readelf -p .comment target/debug/mold_check)
    if ! grep -q "mold" <<<"$comment"; then
      echo "the binary was not linked with mold"
      echo "$comment"
      exit 1
    fi
  '';
}
