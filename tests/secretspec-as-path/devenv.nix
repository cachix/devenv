{ config, pkgs, ... }:

{
  # The .env file feeds secretspec's dotenv provider, not the dotenv integration.
  dotenv.disableHint = true;

  outputs.test-shells = pkgs.buildEnv {
    name = "secret-lease-test-shells";
    paths = [
      pkgs.direnv
      pkgs.zsh
      pkgs.fish
    ];
  };

  env.TLS_KEY_PATH = config.secretspec.secrets.TLS_KEY or "";

  processes.secret-reader.exec = ''
    sleep 1
    test "$(cat "$TLS_KEY_PATH")" = "production-key"
    printf '%s\n' "$TLS_KEY_PATH" > "${config.devenv.root}/daemon-key-path"
    sleep infinity
  '';
}
