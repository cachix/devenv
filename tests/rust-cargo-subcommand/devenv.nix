{ config, ... }:
{
  languages.rust.enable = true;
  env.CARGO_HOME = "${config.devenv.state}/cargo-home-precedence-test";

  enterTest = ''
    workdir=$(mktemp -d)
    preferred_bin="$workdir/preferred-bin"
    mkdir -p "$preferred_bin" "$CARGO_HOME/bin"

    # Simulate the same Cargo subcommand installed both by the environment and
    # by the user in CARGO_HOME/bin. The earlier PATH entry must take precedence.
    printf '%s\n' '#!/bin/sh' 'echo preferred' \
      > "$preferred_bin/cargo-devenv-path-test"
    printf '%s\n' '#!/bin/sh' 'echo cargo-home' \
      > "$CARGO_HOME/bin/cargo-devenv-path-test"
    chmod +x \
      "$preferred_bin/cargo-devenv-path-test" \
      "$CARGO_HOME/bin/cargo-devenv-path-test"

    subcommand_source=$(PATH="$preferred_bin:$PATH" cargo devenv-path-test)
    if [ "$subcommand_source" != preferred ]; then
      echo "cargo preferred the user-installed command in CARGO_HOME/bin"
      exit 1
    fi
  '';
}
