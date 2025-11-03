{
  languages.rust = {
    enable = true;
    toolchainFile = ./rust-toolchain.toml;
  };

  enterTest = ''
    # Verify rust toolchain is available
    rustc --version
    cargo --version

    # Verify components from rust-toolchain.toml are available
    rustfmt --version
    cargo clippy --version

    # Verify we're using the rust-overlay stable channel
    # rust-overlay provides more recent versions than nixpkgs
    # Check that rustc version is at least 1.80 (recent stable)
    rustc_version=$(rustc --version | grep -oP '(?<=rustc )[0-9]+\.[0-9]+')
    major=$(echo $rustc_version | cut -d. -f1)
    minor=$(echo $rustc_version | cut -d. -f2)

    if [ "$major" -lt 1 ] || ([ "$major" -eq 1 ] && [ "$minor" -lt 80 ]); then
      echo "Error: Expected rust-overlay stable (>= 1.80), got $rustc_version"
      exit 1
    fi

    echo "Rust toolchain from rust-toolchain.toml loaded successfully (version $rustc_version)"
  '';
}
