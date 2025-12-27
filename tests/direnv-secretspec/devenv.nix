{ pkgs, config, ... }:

{
  # Verify secrets are available in Nix config
  enterShell = ''
    echo "Secrets loaded via direnv:"
    echo "TEST_SECRET=${config.secretspec.secrets.TEST_SECRET or "NOT_SET"}"
  '';
}
