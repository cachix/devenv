{ pkgs, config, ... }: {
  languages.rust.enable = true;
  languages.rust.mold.enable = false;

  # Test the cargo2nix import functionality
  languages.rust.import = {
    rustTest = {
      root = ./.;
      workspaceMember = "rust-test";
    };
  };

  # Include the imported package in packages
  packages = [
    config.languages.rust.import.rustTest.package
  ];

  enterTest = ''
    # Test that RUSTFLAGS is not set when mold is disabled
    if [ -n "''${RUSTFLAGS+x}" ]; then
      echo "RUSTFLAGS is set, but it should not be"
      exit 1
    fi

    # Test that cargo and rustc are available
    cargo --version
    rustc --version

    # Test building and running the hello world app
    cargo build
    cargo run

    # Test that the binary was created
    if [ ! -f target/debug/rust-test ]; then
      echo "Binary was not created"
      exit 1
    fi

    # Run the binary directly
    ./target/debug/rust-test

    # Test that the imported binary from cargo2nix is available
    rust-test
  '';
}
